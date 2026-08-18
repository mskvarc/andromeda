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
    notification_plan::{NotificationGroup, NotificationPlan},
    side_effect::BatchSideEffectMode,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::WriteOutcome,
    write_retry::WRITE_ATTEMPTS,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, removed_instance::removed_attribute_instances},
    notification::NotificationTrigger,
};
use serde_json::Value;
use std::sync::Arc;

/// One entity's place in a chunk: the change to make and everything the change
/// implies once it lands.
pub(super) struct ChunkWrite {
    /// Entity identifier.
    pub id: Uri,
    /// What happens to the entity row.
    pub entity: EntityMutation,
    /// The payload whose attributes were observed. For a partial update this
    /// is the request fragment, not the entity the fragment produced.
    pub observed: Arc<Value>,
    /// What the write assumes about the entity it was computed from.
    pub precondition: Precondition,
    /// Tombstones the write implies beyond its observations.
    pub tombstones: Vec<HistoryRecord>,
    /// Both notification channels this write owes, planned to mirror its
    /// single-entity sibling's triggers.
    pub plan: NotificationPlan,
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
    pub(super) fn chunk_insert(&self, prepared: PreparedEntity, precondition: Precondition) -> ChunkWrite {
        let PreparedEntity {
            id,
            expanded,
            projection,
            changed_attributes,
            expires_at,
            min_attribute_expires_at,
        } = prepared;
        let payload = Arc::new(expanded);
        let types = projection.types.clone(); // clone: the plan carries the names after the record has taken them
        let groups = [NotificationGroup {
            trigger: NotificationTrigger::EntityCreated,
            attributes: changed_attributes,
        }];
        let plan = self.plan_notifications(&id, &types, &groups, Some(&payload), None);
        ChunkWrite {
            id,
            entity: EntityMutation::Insert(EntityRecord::from_parts(
                Arc::clone(&payload), // clone: Arc ref-count bump; the same payload is the observation
                projection,
                expires_at,
                min_attribute_expires_at,
            )),
            observed: payload,
            precondition,
            tombstones: Vec::new(),
            plan,
        }
    }

    /// The write that stores `payload` as a new entity, deriving its columns
    /// from the payload rather than from a preparation pass.
    pub(super) fn chunk_insert_payload(&self, id: Uri, payload: Arc<Value>, changed_attributes: Vec<AttributeName>, precondition: Precondition) -> ChunkWrite {
        let record = EntityRecord::from_expanded(Arc::clone(&payload)); // clone: Arc ref-count bump; the same payload is the observation
        let types = record.projection.types.clone(); // clone: the plan carries the names the record holds
        let groups = [NotificationGroup {
            trigger: NotificationTrigger::EntityCreated,
            attributes: changed_attributes,
        }];
        let plan = self.plan_notifications(&id, &types, &groups, Some(&payload), None);
        ChunkWrite {
            id,
            entity: EntityMutation::Insert(record),
            observed: payload,
            precondition,
            tombstones: Vec::new(),
            plan,
        }
    }

    /// The write that overwrites an existing entity with `merged`, having
    /// observed `observed`, under the per-op notification `groups`.
    ///
    /// The build site chooses the groups so the write mirrors its sibling: a
    /// replacement raises `entityUpdated`, an attribute update raises the
    /// per-attribute triggers, a merge raises `attributeDeleted` for each
    /// removed attribute beside `entityUpdated`. `previous_snapshot` is the
    /// state before the mutation, carried only when a `showChanges` subscriber
    /// needs the diff.
    pub(super) fn chunk_update(
        &self,
        id: Uri,
        previous: &Value,
        merged: Arc<Value>,
        observed: Arc<Value>,
        groups: Vec<NotificationGroup>,
        previous_snapshot: Option<&Value>,
        precondition: Precondition,
    ) -> ChunkWrite {
        let tombstones = removed_attribute_instances(previous, &merged)
            .into_iter()
            .map(HistoryRecord::Tombstone)
            .collect();
        let types = extract_types(&merged);
        let plan = self.plan_notifications(&id, &types, &groups, Some(&merged), previous_snapshot);
        let record = EntityRecord::from_expanded(merged);
        ChunkWrite {
            id,
            entity: EntityMutation::Update(record),
            observed,
            precondition,
            tombstones,
            plan,
        }
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
    /// In `Inline` mode the observations ride the same transaction as the
    /// entity rows and the change events are emitted once it commits. In
    /// `Enqueue` mode both are persisted alongside the entity rows and a worker
    /// replays them, which is what makes `Prefer: respond-async` crash-consistent
    /// rather than merely faster. The durable subscription matches commit with
    /// the entity row in either mode, never deferred to the worker.
    pub(super) async fn commit_chunk(&self, entries: Vec<ChunkWrite>, mode: BatchSideEffectMode) -> Result<ChunkOutcome, EntityError> {
        if entries.is_empty() {
            return Ok(ChunkOutcome::default());
        }

        let mut writes = Vec::with_capacity(entries.len());
        let mut settled: Vec<(Uri, bool, Vec<ChangeEvent>)> = Vec::with_capacity(entries.len());

        for entry in entries {
            let ChunkWrite {
                id,
                entity,
                observed,
                precondition,
                tombstones,
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

            let (write, inline_events) = match mode {
                BatchSideEffectMode::Inline => {
                    let mut history = Vec::with_capacity(tombstones.len() + 1);
                    history.push(HistoryRecord::Observations(observed));
                    history.extend(tombstones);
                    (builder.history(history).build(), events)
                }
                BatchSideEffectMode::Enqueue => {
                    // Tombstones stay inline in both modes: they describe what
                    // the payload just stopped saying, and deferring them would
                    // leave `current_attribute_values` answering `q` filters with a
                    // value the entity no longer has.
                    let job = BatchSideEffectJob {
                        temporal_entities: vec![(entity_id, observed)],
                        change_events: events,
                    };
                    (builder.history(tombstones).side_effects(Some(job)).build(), Vec::new())
                }
            };

            writes.push(write);
            settled.push((id, creates, inline_events));
        }

        let outcomes = self.writes.commit(writes).await?;
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
            self.emit_change(event);
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
