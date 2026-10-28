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
    stored_census::admit_stored_entity,
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
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
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

/// The two states one entity update moves between.
///
/// They belong together because each is read against the other: the tombstones
/// clause 5.6.6 requires, the per-attribute triggers of clause 5.2.12 and the
/// observation of every modified instance all come from the difference between
/// the stored state and the state replacing it.
pub(super) struct UpdatedStates<'a> {
    /// The entity as the store holds it before the write. It is also the state
    /// a deletion filter and `showChanges` are evaluated against.
    pub previous: &'a Arc<EntityState>,
    /// The entity the write stores in its place, not yet stamped. It shares
    /// every attribute it did not change with `previous`.
    pub current: EntityState,
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
        let UpdatedStates { previous, mut current } = states;
        // Clause 4.8: an updated instance takes `now` for modifiedAt and keeps
        // its createdAt; an untouched one keeps both. Stamped before the columns
        // and the notification plan read the state, so both see it; only an
        // attribute whose stamps change stops being shared with `previous`.
        current.settle(Some(previous), now);
        let current = Arc::new(current);
        // Clauses 5.6.8, 5.6.9 and 5.6.20 run the single-entity behaviours per
        // entity, so the batch records exactly what its single-entity sibling
        // records: the instances it modified and the ends of what it removed.
        let history = mutation_history(previous, &current);
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
        state.settle(None, now);
        let state = Arc::new(state);
        let plan = self.notifier.plan(target, &id, state.types().as_slice(), creation(&state), Some(&state), None);
        // clone: reference-count bump; the stored state is also the write's observation.
        let history = vec![HistoryRecord::Observations(Arc::clone(&state))];
        let record = match EntityRecord::from_state(state) {
            Ok(record) => record,
            Err(error) => return Err(Box::new((id, error.into()))),
        };
        Ok(ChunkWrite {
            id,
            entity: EntityMutation::Insert(record),
            history,
            plan,
        })
    }

    /// The write that stores the state `states` computed in place of the one
    /// it was computed from, or no write when it modified nothing.
    ///
    /// The change test is the single-entity one
    /// ([`leaves_entity_unchanged`](crate::modification_write::leaves_entity_unchanged)):
    /// clause 5.6.8.4 runs Update Attributes or Replace Entity per entity, and
    /// clauses 5.6.9.4 and 5.6.20.4 run Append Attributes and Merge Entity, so
    /// an entity whose fragment or replacement modifies nothing keeps its
    /// `modifiedAt` (clause 4.8), notifies no subscriber and records no
    /// Temporal Evolution instance, exactly as the single-entity operation.
    /// The entity is still settled as a success, because the operation it
    /// stands for succeeded. The write is computed from `previous`, so the
    /// version it holds against is stated by the read-modify-write driver that
    /// read it.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, when the stored entity would pass the
    /// entity value limit or its search columns cannot be derived. Only that
    /// entity is refused (clause 5.6.8.4).
    pub(super) fn chunk_update(
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
        if let Err(exceeded) = admit_stored_entity(self.payload_budget.json_values, Some(previous), &current) {
            return Err(Box::new((id, exceeded.into())));
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
    /// on the live branch, the commit is a respond-async one: the history is
    /// committed as queued intents beside the entity rows, which every
    /// temporal read answers at once, and the change events no durable match
    /// was recorded for are persisted in the same transaction as the job a
    /// worker replays, which is what makes `Prefer: respond-async`
    /// crash-consistent rather than merely faster. The durable subscription
    /// matches commit with the entity row either way, never deferred to the
    /// worker. `accepted_at` is the instant the chunk's writes settled their
    /// system times at, which the commit records their history at.
    pub(super) async fn commit_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        accepted_at: SystemInstant,
        entries: Vec<GuardedWrite>,
    ) -> Result<ChunkOutcome, EntityError> {
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

            let builder = AtomicWrite::builder()
                .entity_id(Arc::new(id.clone())) // clone: the write and the settled outcome each own the identifier.
                .entity(entity)
                .history(history)
                .subscription_matches(matches)
                .precondition(precondition);

            let (write, inline_events) = match placement {
                SideEffectPlacement::Inline(_) => (builder.build(), events),
                SideEffectPlacement::DeferredOnLive => {
                    let job = BatchSideEffectJob {
                        change_events: self.notifier.owed_change_events(events),
                    };
                    (builder.side_effects((!job.is_empty()).then_some(job)).build(), Vec::new())
                }
            };

            writes.push(write);
            settled.push((id, creates, inline_events));
        }

        let outcomes = match placement {
            SideEffectPlacement::Inline(target) => self.writes.commit(target, accepted_at, writes).await?,
            SideEffectPlacement::DeferredOnLive => self.writes.commit_enqueued(accepted_at, writes).await?,
        };
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

        self.notifier.publish(target, emit_events).await;

        Ok(chunk)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
        let error = error_of(&id, WriteOutcome::Refused(SubmissionRefusal::RelationshipMissingObject));
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
