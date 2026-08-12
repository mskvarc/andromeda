//! Committing one chunk of a batch, and settling what each entity in it did.
//!
//! Every batch path funnels through here, which is what makes the per-entity
//! reporting clauses 5.6.7 to 5.6.10 require one rule rather than five: the
//! coordinator answers positionally, so an entity's outcome is read straight
//! off the write that was submitted for it.

use crate::{EntityService, batch::prepare::PreparedEntity, error::EntityError, side_effect::BatchSideEffectMode};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    entity_mutation::{EntityMutation, EntityRecord},
    history_record::HistoryRecord,
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::WriteOutcome,
    write_retry::WRITE_ATTEMPTS,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::removed_instance::removed_attribute_instances,
    notification::{ChangeOrigin, NotificationTrigger},
};
use chrono::Utc;
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
    /// Attribute names this request changed.
    pub changed_attributes: Vec<AttributeName>,
    /// The entity's types, as they will stand after the write.
    pub types: Vec<EntityTypeName>,
    /// What the write assumes about the entity it was computed from.
    pub precondition: Precondition,
    /// Tombstones the write implies beyond its observations.
    pub tombstones: Vec<HistoryRecord>,
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

impl ChunkWrite {
    /// The write that stores a freshly prepared payload as a new entity.
    pub(super) fn insert(prepared: PreparedEntity, precondition: Precondition) -> ChunkWrite {
        let PreparedEntity {
            id,
            expanded,
            types,
            scopes,
            changed_attributes,
            location,
            observation_space,
            operation_space,
            expires_at,
        } = prepared;
        let payload = Arc::new(expanded);
        ChunkWrite {
            id,
            entity: EntityMutation::Insert(EntityRecord {
                payload: Arc::clone(&payload), // clone: Arc ref-count bump; the same payload is the observation
                types: types.clone(),          // clone: the change event carries the names after the record has taken them
                scopes,
                location,
                observation_space,
                operation_space,
                expires_at,
            }),
            observed: payload,
            changed_attributes,
            types,
            precondition,
            tombstones: Vec::new(),
        }
    }

    /// The write that stores `payload` as a new entity, deriving its columns
    /// from the payload rather than from a preparation pass.
    pub(super) fn insert_payload(id: Uri, payload: Arc<Value>, changed_attributes: Vec<AttributeName>, precondition: Precondition) -> ChunkWrite {
        let record = EntityRecord::from_expanded(Arc::clone(&payload)); // clone: Arc ref-count bump; the same payload is the observation
        ChunkWrite {
            id,
            types: record.types.clone(), // clone: the change event carries the names the record holds
            entity: EntityMutation::Insert(record),
            observed: payload,
            changed_attributes,
            precondition,
            tombstones: Vec::new(),
        }
    }

    /// The write that overwrites an existing entity with `merged`, having
    /// observed `observed`.
    pub(super) fn update(
        id: Uri,
        previous: &Value,
        merged: Arc<Value>,
        observed: Arc<Value>,
        changed_attributes: Vec<AttributeName>,
        precondition: Precondition,
    ) -> ChunkWrite {
        let tombstones = removed_attribute_instances(previous, &merged)
            .into_iter()
            .map(HistoryRecord::Tombstone)
            .collect();
        let record = EntityRecord::from_expanded(merged);
        ChunkWrite {
            id,
            types: record.types.clone(), // clone: the change event carries the names the record holds
            entity: EntityMutation::Update(record),
            observed,
            changed_attributes,
            precondition,
            tombstones,
        }
    }
}

/// The change a mutation describes to a subscriber.
const fn trigger_of(entity: &EntityMutation) -> NotificationTrigger {
    match entity {
        EntityMutation::Insert(_) => NotificationTrigger::EntityCreated,
        EntityMutation::Update(_) | EntityMutation::None => NotificationTrigger::EntityUpdated,
        EntityMutation::Delete => NotificationTrigger::EntityDeleted,
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

/// The change event this entity raises.
fn change_event(id: &Uri, types: Vec<EntityTypeName>, changed: Vec<AttributeName>, payload: Arc<Value>, trigger: NotificationTrigger) -> ChangeEvent {
    ChangeEvent::builder()
        .entity_id(id.clone()) // clone: the event owns its identifier
        .entity_types(types)
        .trigger_reason(trigger)
        .occurred_at(Utc::now())
        .origin(ChangeOrigin::Local)
        .changed_attributes(changed)
        .current_payload(Some(payload))
        .build()
}

impl EntityService {
    /// Submit one chunk's writes and settle what each entity did.
    ///
    /// In `Inline` mode the observations ride the same transaction as the
    /// entity rows and the change events are emitted once it commits. In
    /// `Enqueue` mode both are persisted alongside the entity rows and a worker
    /// replays them, which is what makes `Prefer: respond-async` crash-consistent
    /// rather than merely faster.
    pub(super) async fn commit_chunk(&self, entries: Vec<ChunkWrite>, mode: BatchSideEffectMode) -> Result<ChunkOutcome, EntityError> {
        if entries.is_empty() {
            return Ok(ChunkOutcome::default());
        }

        let mut writes = Vec::with_capacity(entries.len());
        let mut settled: Vec<(Uri, bool, Option<ChangeEvent>)> = Vec::with_capacity(entries.len());

        for entry in entries {
            let ChunkWrite {
                id,
                entity,
                observed,
                changed_attributes,
                types,
                precondition,
                tombstones,
            } = entry;
            let creates = entity.is_insert();
            let event = Some(change_event(&id, types, changed_attributes, Arc::clone(&observed), trigger_of(&entity))); // clone: Arc ref-count bump
            let entity_id = Arc::new(id.clone()); // clone: the write and deferred side effect share this identifier.

            let builder = AtomicWrite::builder()
                .entity_id(Arc::clone(&entity_id)) // clone: Arc ref-count bump
                .entity(entity)
                .precondition(precondition);

            let (write, inline_event) = match mode {
                BatchSideEffectMode::Inline => {
                    let mut history = Vec::with_capacity(tombstones.len() + 1);
                    history.push(HistoryRecord::Observations(observed));
                    history.extend(tombstones);
                    (builder.history(history).build(), event)
                }
                BatchSideEffectMode::Enqueue => {
                    // Tombstones stay inline in both modes: they describe what
                    // the payload just stopped saying, and deferring them would
                    // leave `current_attribute_values` answering `q` filters with a
                    // value the entity no longer has.
                    let job = BatchSideEffectJob {
                        temporal_entities: vec![(entity_id, observed)],
                        change_events: event.into_iter().collect(),
                    };
                    (builder.history(tombstones).side_effects(Some(job)).build(), None)
                }
            };

            writes.push(write);
            settled.push((id, creates, inline_event));
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
        let mut events = Vec::with_capacity(settled.len());
        for ((id, creates, event), outcome) in settled.into_iter().zip(&outcomes) {
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
            events.extend(event);
        }

        for event in events {
            self.emit_change(event);
        }

        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::{error_of, trigger_of};
    use andromeda_context_commit::{
        entity_mutation::{EntityMutation, EntityRecord},
        write_outcome::WriteOutcome,
    };
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use serde_json::json;
    use std::sync::Arc;

    fn record() -> EntityRecord {
        EntityRecord::from_expanded(Arc::new(json!([{"@id": "urn:ngsi-ld:A"}])))
    }

    #[test]
    fn each_mutation_describes_one_trigger() {
        pretty_assertions::assert_eq!(trigger_of(&EntityMutation::Insert(record())), NotificationTrigger::EntityCreated);
        pretty_assertions::assert_eq!(trigger_of(&EntityMutation::Update(record())), NotificationTrigger::EntityUpdated);
        pretty_assertions::assert_eq!(trigger_of(&EntityMutation::Delete), NotificationTrigger::EntityDeleted);
    }

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
