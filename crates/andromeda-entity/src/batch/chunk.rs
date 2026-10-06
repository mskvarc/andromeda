//! Committing one chunk of a batch, and settling what each entity in it did.
//!
//! Every batch path funnels through here, which is what makes the per-entity
//! reporting clauses 5.6.7 to 5.6.10 require one rule rather than five: the
//! coordinator answers positionally, so an entity's outcome is read straight
//! off the write that was submitted for it.
//!
//! Each write also carries a [`NotificationPlan`] its build site computed to
//! mirror the corresponding single-entity mutation's triggers (clause 5.2.12).
//! The durable matches in it commit in the same transaction as the entity row,
//! on both the inline and the asynchronous path; the in-process events are
//! emitted after the commit inline, or replayed from the durable job
//! asynchronously.

use crate::{
    EntityService,
    entity_invariant::EntityInvariant,
    error::EntityError,
    modification_write::leaves_entity_unchanged,
    notification_plan::{NotificationPlan, creation, modification},
    side_effect::SideEffectPlacement,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    error::CommitError,
    history_record::{HistoryRecord, mutation_history},
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::WriteOutcome,
    write_retry::WRITE_ATTEMPTS,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, history::written_observation::AddressedInstances, system_instant::SystemInstant};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::sync::Arc;

/// One entity's place in a chunk: the change to make and everything the change
/// implies once it lands.
pub(super) struct ChunkWrite {
    /// Entity identifier.
    pub id: Identifier,
    /// What happens to the entity row.
    pub entity: EntityMutation,
    /// What the write records in the Temporal Evolution: the observation of
    /// the instances it wrote, as the stored Entity holds them, and the ends
    /// of the instances and Scope it removed.
    pub history: Vec<HistoryRecord>,
    /// Both notification channels this write owes, planned to mirror its
    /// single-entity sibling's triggers.
    pub plan: NotificationPlan,
}

/// A chunk write together with what it assumes about the entity it was
/// computed from.
pub(super) struct GuardedWrite {
    /// The write to commit.
    pub write: ChunkWrite,
    /// The condition the store checks before it applies the write.
    pub precondition: Precondition,
}

/// The two states one entity update moves between, and what it addressed.
///
/// They belong together because each is read against the others: the tombstones
/// clause 5.6.6 requires and the per-attribute triggers of clause 5.2.12 both
/// come from the difference between the stored state and the state replacing
/// it, and the observation is the resulting state of the instances the request
/// addressed.
pub(super) struct UpdatedStates<'a> {
    /// The entity as the store holds it before the write. It is also the state
    /// a deletion filter and `showChanges` are evaluated against.
    pub previous: &'a Arc<EntityState>,
    /// The entity the write stores in its place, not yet stamped. It shares
    /// every attribute it did not change with `previous`.
    pub current: EntityState,
    /// The instances the request addressed, whose resulting state the
    /// Temporal Evolution records.
    pub addressed: AddressedInstances<'a>,
}

/// An entity update once its instances carry the write instant: the state it
/// replaces, the state replacing it and what the Temporal Evolution records.
struct StampedUpdate<'a> {
    previous: &'a Arc<EntityState>,
    current: Arc<EntityState>,
    history: Vec<HistoryRecord>,
}

impl<'a> StampedUpdate<'a> {
    /// Stamp the states of `states` with `now` and derive the history.
    fn of(states: UpdatedStates<'a>, now: SystemInstant) -> StampedUpdate<'a> {
        let UpdatedStates {
            previous,
            mut current,
            addressed,
        } = states;
        // Clause 4.8: an updated instance takes `now` for modifiedAt and keeps
        // its createdAt; an untouched one keeps both. Stamped before the columns
        // and the notification plan read the state, so both see it; only an
        // attribute whose stamps change stops being shared with `previous`.
        current.stamp_attribute_instants(Some(previous), now);
        let current = Arc::new(current);
        // Clauses 5.6.8, 5.6.9 and 5.6.20 run the single-entity behaviours per
        // entity, so the batch records exactly what its single-entity sibling
        // records: the resulting instances it addressed and the ends of what it
        // removed.
        let history = mutation_history(addressed, previous, &current);
        StampedUpdate { previous, current, history }
    }
}

/// What one chunk reports back to its batch.
#[derive(Debug, Default)]
pub(super) struct ChunkOutcome {
    /// Identifiers whose write landed.
    pub success: Vec<Identifier>,
    /// Identifiers whose write created the entity (clause 5.6.8 reports these
    /// separately so the response can carry 201).
    pub created: Vec<Identifier>,
    /// Per-entity failures a re-read cannot fix.
    pub errors: Vec<(Identifier, EntityError)>,
    /// Refusals a caller that read the entity first can fix by reading again:
    /// the entity moved on, or an identifier it read as absent now exists.
    /// A caller that read nothing has no reason to retry and folds these into
    /// [`Self::errors`] with [`Self::settled`].
    pub retryable: Vec<(Identifier, EntityError)>,
}

impl ChunkOutcome {
    /// Treat every refusal as final.
    pub(super) fn settled(mut self) -> ChunkOutcome {
        self.errors.append(&mut self.retryable);
        self
    }
}

impl EntityService {
    /// The write that stores `state` as a new entity.
    ///
    /// Mirrors the single create (clause 5.6.1): one `entityCreated` over the
    /// whole entity, and the whole entity as its first observation. A batch
    /// inserts only an entity it was asked to create or found absent, so the
    /// write is committed to hold only while the entity does not exist.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, when the entity's search columns cannot be
    /// derived.
    pub(super) fn chunk_insert(
        &self,
        target: &ContextTarget,
        id: Identifier,
        mut state: EntityState,
        now: SystemInstant,
    ) -> Result<ChunkWrite, Box<(Identifier, EntityError)>> {
        // Clause 4.8: a created entity's attribute instances carry the write
        // instant as both createdAt and modifiedAt.
        state.stamp_attribute_instants(None, now);
        let state = Arc::new(state);
        let plan = self.notifier.plan(target, &id, state.types().as_slice(), creation(&state), Some(&state), None);
        let record = match EntityRecord::from_state(Arc::clone(&state)) {
            // clone: reference-count bump; the same state is the observation
            Ok(record) => record,
            Err(error) => return Err(Box::new((id, error.into()))),
        };
        Ok(ChunkWrite {
            id,
            entity: EntityMutation::Insert(record),
            history: vec![HistoryRecord::Observations(state)],
            plan,
        })
    }

    /// The write that overwrites an existing entity with the state replacing
    /// it, whether or not the replacement differs from what is stored.
    ///
    /// The per-attribute triggers of clause 5.2.12 are read off the difference
    /// between the two states. The write is computed from `previous`, so the
    /// version it holds against is stated by the read-modify-write driver
    /// that read it.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, when the entity's search columns cannot be
    /// derived.
    pub(super) fn chunk_update(
        &self,
        target: &ContextTarget,
        id: Identifier,
        states: UpdatedStates<'_>,
        now: SystemInstant,
    ) -> Result<ChunkWrite, Box<(Identifier, EntityError)>> {
        let StampedUpdate { previous, current, history } = StampedUpdate::of(states, now);
        self.update_write(target, id, previous, current, history)
    }

    /// The write a fragment operation owes for turning an entity into the
    /// state `states` computed, or no write when the fragment changed nothing.
    ///
    /// The change test is the single-entity one
    /// ([`leaves_entity_unchanged`](crate::modification_write::leaves_entity_unchanged)):
    /// clauses 5.6.8.4 (update mode), 5.6.9.4 and 5.6.20.4 run Update
    /// Attributes, Append Attributes and Merge Entity per entity, so an entity
    /// whose fragment names nothing to change keeps its `modifiedAt` (clause
    /// 4.8), notifies no subscriber and records no Temporal Evolution
    /// instance, exactly as the single-entity operation. The entity is still
    /// settled as a success, because the operation it stands for succeeded.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, when the entity's search columns cannot be
    /// derived.
    pub(super) fn chunk_fragment_update(
        &self,
        target: &ContextTarget,
        id: Identifier,
        states: UpdatedStates<'_>,
        now: SystemInstant,
    ) -> Result<Option<ChunkWrite>, Box<(Identifier, EntityError)>> {
        let StampedUpdate { previous, current, history } = StampedUpdate::of(states, now);
        if leaves_entity_unchanged(previous, &current, &history) {
            return Ok(None);
        }
        self.update_write(target, id, previous, current, history).map(Some)
    }

    /// The write storing `current` in place of `previous`, with the
    /// notifications and history it owes.
    fn update_write(
        &self,
        target: &ContextTarget,
        id: Identifier,
        previous: &Arc<EntityState>,
        current: Arc<EntityState>,
        history: Vec<HistoryRecord>,
    ) -> Result<ChunkWrite, Box<(Identifier, EntityError)>> {
        let plan = self.notifier.plan(
            target,
            &id,
            current.types().as_slice(),
            modification(previous, &current),
            Some(&current),
            Some(previous),
        );
        let record = match EntityRecord::replacing(previous, current) {
            Ok(record) => record,
            Err(error) => return Err(Box::new((id, error.into()))),
        };
        Ok(ChunkWrite {
            id,
            entity: EntityMutation::Update(record),
            history,
            plan,
        })
    }
}

/// The per-entity error an outcome reports, if it reports one.
///
/// `Skipped` answers only an unconditional insert, which no batch path issues.
/// A write whose content the store refused reports that refusal against its
/// own entity only (clause 5.6.8.4: "If while processing an Entity there is
/// any kind of error or abnormal situation, a `BatchEntityError` shall be
/// added").
fn error_of(id: &Identifier, outcome: WriteOutcome) -> Option<EntityError> {
    match outcome {
        WriteOutcome::AlreadyExists => Some(EntityError::already_exists(id)),
        WriteOutcome::NotFound => Some(EntityError::not_found(id)),
        WriteOutcome::Superseded => Some(EntityError::contention(id, WRITE_ATTEMPTS)),
        WriteOutcome::Refused(refusal) => Some(EntityError::from(CommitError::Submission(refusal))),
        WriteOutcome::Created | WriteOutcome::Updated | WriteOutcome::Deleted(_) | WriteOutcome::NoEntityChange | WriteOutcome::Skipped => None,
    }
}

impl EntityService {
    /// Submit one chunk's writes and settle what each entity did.
    ///
    /// Placed inline, the observations ride the same transaction as the
    /// entity rows and the change events are emitted once it commits. Deferred
    /// on the live branch, both are persisted alongside the entity rows and a
    /// worker replays them, which is what makes `Prefer: respond-async`
    /// crash-consistent rather than merely faster. The durable subscription
    /// matches commit with the entity row either way, never deferred to the
    /// worker, and a job carries the change events only when no durable match
    /// was recorded for them.
    pub(super) async fn commit_chunk(&self, placement: SideEffectPlacement<'_>, entries: Vec<GuardedWrite>) -> Result<ChunkOutcome, EntityError> {
        let target = placement.target();
        if entries.is_empty() {
            return Ok(ChunkOutcome::default());
        }

        let mut writes = Vec::with_capacity(entries.len());
        let mut settled: Vec<(Identifier, bool, Vec<ChangeEvent>)> = Vec::with_capacity(entries.len());

        for GuardedWrite { write, precondition } in entries {
            let ChunkWrite { id, entity, history, plan } = write;
            let NotificationPlan { matches, events } = plan;
            let creates = entity.is_insert();
            let entity_id = Arc::new(id.clone()); // clone: the write and deferred side effect share this identifier.

            let builder = AtomicWrite::builder()
                .entity_id(Arc::clone(&entity_id)) // clone: Arc ref-count bump
                .entity(entity)
                .subscription_matches(matches)
                .precondition(precondition);

            let (write, inline_events) = match placement {
                SideEffectPlacement::Inline(_) => (builder.history(history).build(), events),
                SideEffectPlacement::DeferredOnLive => {
                    // Tombstones stay inline in both modes: they describe what
                    // the state just stopped saying, and deferring them would
                    // leave `current_attribute_values` answering `q` filters with a
                    // value the entity no longer has.
                    let (observations, endings): (Vec<HistoryRecord>, Vec<HistoryRecord>) = history.into_iter().partition(HistoryRecord::is_observations);
                    let temporal_entities = observations
                        .into_iter()
                        .filter_map(|record| match record {
                            HistoryRecord::Observations(observed) => Some((Arc::clone(&entity_id), observed)), // clone: Arc ref-count bump
                            // A Core batch writes no Temporal API addition, and the
                            // partition above routes none here.
                            HistoryRecord::EvolutionInstances(_) | HistoryRecord::Tombstone(_) | HistoryRecord::ScopeRemoval => None,
                        })
                        .collect();
                    let job = BatchSideEffectJob {
                        temporal_entities,
                        change_events: self.notifier.owed_change_events(events),
                    };
                    (builder.history(endings).side_effects((!job.is_empty()).then_some(job)).build(), Vec::new())
                }
            };

            writes.push(write);
            settled.push((id, creates, inline_events));
        }

        let outcomes = self.writes.commit(target, writes).await?;
        if outcomes.len() != settled.len() {
            return Err(EntityInvariant::OutcomeCountMismatch {
                submitted: settled.len(),
                returned: outcomes.len(),
            }
            .into());
        }

        let mut chunk = ChunkOutcome::default();
        let mut emit_events = Vec::new();
        for ((id, creates, events), outcome) in settled.into_iter().zip(outcomes) {
            let next = after_attempt(&outcome);
            let skipped = outcome.is_skipped();
            if let Some(error) = error_of(&id, outcome) {
                match next {
                    AfterAttempt::Reread => chunk.retryable.push((id, error)),
                    AfterAttempt::Settled | AfterAttempt::Vanished => chunk.errors.push((id, error)),
                }
                continue;
            }
            if skipped {
                continue;
            }
            if creates {
                chunk.created.push(id.clone()); // clone: upsert reports a created identifier in both result lists
            }
            chunk.success.push(id);
            emit_events.extend(events);
        }

        for event in emit_events {
            self.notifier.publish(target, event);
        }

        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    use crate::{batch::chunk::error_of, error::EntityError};
    use andromeda_context_commit::{submission_refusal::SubmissionRefusal, write_outcome::WriteOutcome};

    #[test]
    fn a_conflicting_insert_reports_409_and_a_missing_update_reports_404() {
        // NGSI-LD 6.4.3.1 for the conflict; clause 5.6.9 for the absent entity.
        let id = "urn:ngsi-ld:A".parse().expect("URI");
        assert!(error_of(&id, WriteOutcome::AlreadyExists).is_some_and(|error| !error.is_not_found()));
        assert!(error_of(&id, WriteOutcome::NotFound).is_some_and(|error| error.is_not_found()));
    }

    /// A refusal of the entity's own content is a validation failure of that
    /// entity, answered `BadRequestData` in its `BatchEntityError`.
    #[test]
    fn a_refused_write_reports_its_refusal_as_a_validation_failure() {
        let id = "urn:ngsi-ld:A".parse().expect("URI");
        let error = error_of(&id, WriteOutcome::Refused(SubmissionRefusal::UnitCodeUnregistered));
        assert!(error.as_ref().is_some_and(EntityError::is_validation), "{error:?}");
    }

    #[test]
    fn a_landed_or_skipped_outcome_reports_no_error() {
        for outcome in [
            WriteOutcome::Created,
            WriteOutcome::Updated,
            WriteOutcome::NoEntityChange,
            WriteOutcome::Skipped,
        ] {
            let id = "urn:ngsi-ld:A".parse().expect("URI");
            assert!(error_of(&id, outcome).is_none());
        }
    }
}
