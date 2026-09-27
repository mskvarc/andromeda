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
    batch::prepare::PreparedEntity,
    error::EntityError,
    expanded_member::written_entity_types,
    notification_plan::{NotificationGroup, NotificationPlan, attribute_change_groups},
    side_effect::SideEffectPlacement,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::{HistoryRecord, mutation_history},
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::WriteOutcome,
    write_retry::WRITE_ATTEMPTS,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{attribute_system_time::stamp_attribute_instants, mutation::error::ExpandedEntityError, written_observation::AddressedInstances},
    notification::NotificationTrigger,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// One entity's place in a chunk: the change to make and everything the change
/// implies once it lands.
pub(super) struct ChunkWrite {
    /// Entity identifier.
    pub id: Uri,
    /// What happens to the entity row.
    pub entity: EntityMutation,
    /// What the write records in the Temporal Evolution: the observation of
    /// the instances it wrote, as the stored Entity holds them, and the ends
    /// of the instances and Scope it removed.
    pub history: Vec<HistoryRecord>,
    /// What the write assumes about the entity it was computed from.
    pub precondition: Precondition,
    /// Both notification channels this write owes, planned to mirror its
    /// single-entity sibling's triggers.
    pub plan: NotificationPlan,
}

/// The two states one entity update moves between, and what it addressed.
///
/// They belong together because each is read against the others: the tombstones
/// clause 5.6.6 requires and the per-attribute triggers of clause 5.2.12 both
/// come from the difference between the stored state and the state replacing
/// it, and the observation is the resulting state of the instances the request
/// addressed.
pub(super) struct UpdatedPayloads<'a> {
    /// The entity as the store holds it before the write. It is also the state
    /// a deletion filter and `showChanges` are evaluated against.
    pub previous: &'a Value,
    /// The entity the write stores in its place.
    pub merged: Arc<Value>,
    /// The instances the request addressed, whose resulting state the
    /// Temporal Evolution records.
    pub addressed: AddressedInstances<'a>,
}

/// What one chunk reports back to its batch.
#[derive(Debug, Default)]
pub(super) struct ChunkOutcome {
    /// Identifiers whose write landed.
    pub success: Vec<Uri>,
    /// Identifiers whose write created the entity (clause 5.6.8 reports these
    /// separately so the response can carry 201).
    pub created: Vec<Uri>,
    /// Per-entity failures a re-read cannot fix.
    pub errors: Vec<(Uri, EntityError)>,
    /// Refusals a caller that read the entity first can fix by reading again:
    /// the entity moved on, or an identifier it read as absent now exists.
    /// A caller that read nothing has no reason to retry and folds these into
    /// [`Self::errors`] with [`Self::settled`]. Typed because the retry path
    /// reads the store again by identifier, which the port keys on [`Uri`].
    pub retryable: Vec<(Uri, EntityError)>,
}

impl ChunkOutcome {
    /// Treat every refusal as final.
    pub(super) fn settled(mut self) -> ChunkOutcome {
        self.errors.append(&mut self.retryable);
        self
    }
}

impl EntityService {
    /// The write that stores a freshly prepared payload as a new entity.
    ///
    /// Mirrors the single create (clause 5.6.1): one `entityCreated` over the
    /// whole payload.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report, when an instance's `datasetId` cannot be read.
    pub(super) fn chunk_insert(
        &self,
        target: &ContextTarget,
        prepared: PreparedEntity,
        precondition: Precondition,
        now: DateTime<Utc>,
    ) -> Result<ChunkWrite, Box<(Uri, EntityError)>> {
        let PreparedEntity {
            id,
            mut expanded,
            projection,
            changed_attributes,
            expires_at,
            min_attribute_expires_at,
        } = prepared;
        // Clause 4.8: a created entity's attribute instances carry the write
        // instant as both createdAt and modifiedAt.
        if let Err(error) = stamp_attribute_instants(&mut expanded, &Value::Null, now) {
            return Err(Box::new((id, ExpandedEntityError::from(error).into())));
        }
        let payload = Arc::new(expanded);
        let types = projection.types.clone(); // clone: the plan carries the names after the record has taken them
        let mut groups = vec![NotificationGroup::entity(NotificationTrigger::EntityCreated, changed_attributes)];
        match attribute_change_groups(&Value::Null, &payload) {
            Ok(changes) => groups.extend(changes),
            Err(error) => return Err(Box::new((id, error))),
        }
        let plan = self.plan_notifications(target, &id, &types, &groups, Some(&payload), None);
        Ok(ChunkWrite {
            id,
            entity: EntityMutation::Insert(EntityRecord::from_parts(
                Arc::clone(&payload), // clone: Arc ref-count bump; the same payload is the observation
                projection,
                expires_at,
                min_attribute_expires_at,
            )),
            history: vec![HistoryRecord::Observations(payload)],
            precondition,
            plan,
        })
    }

    /// The write that stores `payload` as a new entity, deriving its columns
    /// from the payload rather than from a preparation pass.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report, when the payload's members cannot be read.
    pub(super) fn chunk_insert_payload(
        &self,
        target: &ContextTarget,
        id: Uri,
        payload: Arc<Value>,
        changed_attributes: Vec<AttributeName>,
        precondition: Precondition,
        now: DateTime<Utc>,
    ) -> Result<ChunkWrite, Box<(Uri, EntityError)>> {
        // Clause 4.8: stamp the created entity's attribute instances before the
        // record derives its columns from the payload.
        let mut payload_value = Arc::try_unwrap(payload).unwrap_or_else(|shared| (*shared).clone()); // clone: only when another reference still holds the payload
        if let Err(error) = stamp_attribute_instants(&mut payload_value, &Value::Null, now) {
            return Err(Box::new((id, ExpandedEntityError::from(error).into())));
        }
        let payload = Arc::new(payload_value);
        let record = match EntityRecord::from_expanded(Arc::clone(&payload)) {
            // clone: Arc ref-count bump; the same payload is the observation
            Ok(record) => record,
            Err(error) => return Err(Box::new((id, error.into()))),
        };
        let types = record.projection.types.clone(); // clone: the plan carries the names the record holds
        let mut groups = vec![NotificationGroup::entity(NotificationTrigger::EntityCreated, changed_attributes)];
        match attribute_change_groups(&Value::Null, &payload) {
            Ok(changes) => groups.extend(changes),
            Err(error) => return Err(Box::new((id, error))),
        }
        let plan = self.plan_notifications(target, &id, &types, &groups, Some(&payload), None);
        Ok(ChunkWrite {
            id,
            entity: EntityMutation::Insert(record),
            history: vec![HistoryRecord::Observations(payload)],
            precondition,
            plan,
        })
    }

    /// The write that overwrites an existing entity with the state replacing it,
    /// under the per-op notification `groups`.
    ///
    /// The build site supplies one precise group per changed attribute instance.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report, when the merged payload's members or either state's
    /// `datasetId`s cannot be read.
    pub(super) fn chunk_update(
        &self,
        target: &ContextTarget,
        id: Uri,
        payloads: UpdatedPayloads<'_>,
        groups: &[NotificationGroup],
        precondition: Precondition,
        now: DateTime<Utc>,
    ) -> Result<ChunkWrite, Box<(Uri, EntityError)>> {
        let UpdatedPayloads { previous, merged, addressed } = payloads;
        // Clause 4.8: an updated instance takes `now` for modifiedAt and keeps
        // its createdAt; an untouched one keeps both. Stamped before the columns
        // and the notification plan read the payload, so both see it.
        let mut merged_value = Arc::try_unwrap(merged).unwrap_or_else(|shared| (*shared).clone()); // clone: only when another reference still holds the merged payload
        if let Err(error) = stamp_attribute_instants(&mut merged_value, previous, now) {
            return Err(Box::new((id, ExpandedEntityError::from(error).into())));
        }
        let merged = Arc::new(merged_value);
        // Clauses 5.6.8, 5.6.9 and 5.6.20 run the single-entity behaviours per
        // entity, so the batch records exactly what its single-entity sibling
        // records: the resulting instances it addressed and the ends of what it
        // removed.
        let history = match mutation_history(addressed, previous, &merged) {
            Ok(history) => history,
            Err(error) => return Err(Box::new((id, error.into()))),
        };
        let types = match written_entity_types(&merged) {
            Ok(types) => types,
            Err(error) => return Err(Box::new((id, error))),
        };
        let plan = self.plan_notifications(target, &id, &types, groups, Some(&merged), Some(previous));
        let record = match EntityRecord::from_expanded(merged) {
            Ok(record) => record,
            Err(error) => return Err(Box::new((id, error.into()))),
        };
        Ok(ChunkWrite {
            id,
            entity: EntityMutation::Update(record),
            history,
            precondition,
            plan,
        })
    }
}

/// The per-entity error an outcome reports, if it reports one.
///
/// `Skipped` is silent on purpose: an unconditional insert that met an existing
/// row neither succeeded nor failed, and the upsert path that issues one has
/// always left such entities out of both lists.
fn error_of(id: &Uri, outcome: &WriteOutcome) -> Option<EntityError> {
    match outcome {
        WriteOutcome::AlreadyExists => Some(EntityError::already_exists(id)),
        WriteOutcome::NotFound => Some(EntityError::not_found(id)),
        WriteOutcome::Superseded => Some(EntityError::contention(id, WRITE_ATTEMPTS)),
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
    pub(super) async fn commit_chunk(&self, placement: SideEffectPlacement<'_>, entries: Vec<ChunkWrite>) -> Result<ChunkOutcome, EntityError> {
        let target = placement.target();
        if entries.is_empty() {
            return Ok(ChunkOutcome::default());
        }

        let mut writes = Vec::with_capacity(entries.len());
        let mut settled: Vec<(Uri, bool, Vec<ChangeEvent>)> = Vec::with_capacity(entries.len());

        for entry in entries {
            let ChunkWrite {
                id,
                entity,
                history,
                precondition,
                plan,
            } = entry;
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
                    // the payload just stopped saying, and deferring them would
                    // leave `current_attribute_values` answering `q` filters with a
                    // value the entity no longer has.
                    let (observations, endings): (Vec<HistoryRecord>, Vec<HistoryRecord>) = history.into_iter().partition(HistoryRecord::is_observations);
                    let temporal_entities = observations
                        .into_iter()
                        .filter_map(|record| match record {
                            HistoryRecord::Observations(observed) => Some((Arc::clone(&entity_id), observed)), // clone: Arc ref-count bump
                            HistoryRecord::Tombstone(_) | HistoryRecord::ScopeRemoval => None,
                        })
                        .collect();
                    let job = BatchSideEffectJob {
                        temporal_entities,
                        change_events: self.owed_change_events(events),
                    };
                    (builder.history(endings).side_effects(Some(job)).build(), Vec::new())
                }
            };

            writes.push(write);
            settled.push((id, creates, inline_events));
        }

        let outcomes = self.writes.commit(target, writes).await?;
        if outcomes.len() != settled.len() {
            return Err(EntityError::internal(format!(
                "chunk of {} writes produced {} outcomes",
                settled.len(),
                outcomes.len()
            )));
        }

        let mut chunk = ChunkOutcome::default();
        let mut emit_events = Vec::new();
        for ((id, creates, events), outcome) in settled.into_iter().zip(&outcomes) {
            if let Some(error) = error_of(&id, outcome) {
                match after_attempt(outcome) {
                    AfterAttempt::Reread => chunk.retryable.push((id, error)),
                    AfterAttempt::Settled | AfterAttempt::Vanished => chunk.errors.push((id, error)),
                }
                continue;
            }
            if outcome.is_skipped() {
                continue;
            }
            if creates {
                chunk.created.push(id.clone()); // clone: upsert reports a created identifier in both result lists
            }
            chunk.success.push(id);
            emit_events.extend(events);
        }

        for event in emit_events {
            self.emit_change(target, event);
        }

        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::error_of;
    use andromeda_context_commit::write_outcome::WriteOutcome;

    #[test]
    fn a_conflicting_insert_reports_409_and_a_missing_update_reports_404() {
        // NGSI-LD 6.4.3.1 for the conflict; clause 5.6.9 for the absent entity.
        let id = "urn:ngsi-ld:A".parse().expect("URI");
        assert!(error_of(&id, &WriteOutcome::AlreadyExists).is_some_and(|error| !error.is_not_found()));
        assert!(error_of(&id, &WriteOutcome::NotFound).is_some_and(|error| error.is_not_found()));
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
            assert!(error_of(&id, &outcome).is_none());
        }
    }
}
