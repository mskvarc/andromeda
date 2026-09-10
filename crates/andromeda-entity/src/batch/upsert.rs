//! Batch upsert (clause 5.6.8), in both of its modes.
//!
//! Replace mode overwrites `entity_documents.payload` wholesale, so any attribute the
//! replacement omits is deleted and needs a tombstone. That diff is only
//! meaningful against the payload the entity actually had when the write
//! landed, so each replacement states the `modifiedAt` its diff assumed and is
//! recomputed against a fresh read if a concurrent writer got in first. The
//! diff is taken against `entity_documents.payload` rather than the temporal catalogue
//! on purpose: catalogue rows outlive the attributes they describe, because a
//! deletion writes a tombstone rather than dropping the row, so diffing against
//! them would re-tombstone already-deleted attributes on every upsert.
//!
//! Update mode merges instead of replacing, so only explicit null-sentinel
//! removals imply tombstones.
//!
//! Neither mode sees an identifier twice: clause 5.5.11.2 requires the
//! instances an array repeats to be replaced or updated "in chronological
//! order", so the coordinator hands each one to a separate round, and a chunk
//! carries at most one instance of any entity.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        chunk::{ChunkWrite, UpdatedPayloads},
    },
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    notification_plan::attribute_change_groups,
    side_effect::BatchSideEffectMode,
};
use andromeda_context_commit::{precondition::Precondition, write_retry::WRITE_ATTEMPTS};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{entity_member::extract_attribute_names, mutation::entity::update_entity_attrs},
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, mem::take, sync::Arc};

/// One chunk's per-entity upsert results.
type UpsertOutcome = (BatchResult, Vec<Uri>);

/// One chunk's per-entity upsert results, the failures it collected while
/// preparing them, and the identifiers it created.
pub(super) type UpsertChunkOutcome = (BatchResult, Vec<(Uri, EntityError)>, Vec<Uri>);

impl EntityService {
    pub(super) async fn batch_upsert_replace_chunk(
        &self,
        target: &ContextTarget,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(Uri, EntityError)>,
        mode: BatchSideEffectMode,
    ) -> Result<UpsertOutcome, EntityError> {
        let (prepared, mut chunk_errors) = self.prepare_entity_refs(entities, context).await;
        errors.append(&mut chunk_errors);
        if prepared.is_empty() {
            return Ok((
                BatchResult {
                    success: Vec::new(),
                    errors: Vec::new(),
                },
                Vec::new(),
            ));
        }

        let chunk_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        let _id_guards = self.acquire_id_locks(target, &chunk_ids).await;

        let snapshots = self.reader.get_snapshot_batch(target, &chunk_ids).await?;
        let mut payloads: HashMap<Uri, Arc<Value>> = HashMap::with_capacity(prepared.len());
        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            let write = match snapshots.get(&entity.id) {
                Some(snapshot) => {
                    let payload = Arc::new(entity.expanded);
                    self.replacement_write(target, entity.id, &payload, &snapshot.payload, snapshot.modified_at, entity.changed_attributes)
                }
                None => self.chunk_insert(target, entity, Precondition::MustNotExist),
            };
            // clone: an `Arc` handle kept for a possible retry, which must not
            // re-expand the request payload.
            payloads.insert(write.id.clone(), Arc::clone(&write.observed));
            writes.push(write);
        }

        let mut success = Vec::new();
        let mut created = Vec::new();
        let mut outcome = self.commit_chunk(target, writes, mode).await?;

        for attempt in 1..=WRITE_ATTEMPTS {
            success.extend(take(&mut outcome.success));
            created.extend(take(&mut outcome.created));
            errors.extend(take(&mut outcome.errors));

            let retry: Vec<(Uri, EntityError)> = take(&mut outcome.retryable);
            if retry.is_empty() {
                break;
            }
            if attempt == WRITE_ATTEMPTS {
                errors.extend(retry);
                break;
            }

            self.wait_before_retry(attempt).await;

            let retry_ids: Vec<Uri> = retry.into_iter().map(|(id, _error)| id).collect();
            let snapshots = self.reader.get_snapshot_batch(target, &retry_ids).await?;
            let mut retry_writes = Vec::with_capacity(retry_ids.len());
            for id in retry_ids {
                let Some(payload) = payloads.get(&id) else {
                    continue;
                };
                let changed = extract_attribute_names(payload);
                // clone: `Arc` ref-count bumps; the payload is reused for every
                // attempt this entity needs.
                let write = match snapshots.get(&id) {
                    Some(snapshot) => self.replacement_write(target, id, payload, &snapshot.payload, snapshot.modified_at, changed),
                    None => self.chunk_insert_payload(target, id, Arc::clone(payload), changed, Precondition::MustNotExist),
                };
                retry_writes.push(write);
            }
            outcome = self.commit_chunk(target, retry_writes, mode).await?;
        }

        Ok((BatchResult { success, errors: Vec::new() }, created))
    }

    pub(super) async fn batch_upsert_update_chunk(
        &self,
        target: &ContextTarget,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(Uri, EntityError)>,
        mode: BatchSideEffectMode,
    ) -> Result<UpsertOutcome, EntityError> {
        let (prepared, mut chunk_errors) = self.prepare_entity_refs(entities, context).await;
        errors.append(&mut chunk_errors);
        if prepared.is_empty() {
            return Ok((
                BatchResult {
                    success: Vec::new(),
                    errors: Vec::new(),
                },
                Vec::new(),
            ));
        }

        let prepared_ids: Vec<Uri> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: the guards outlive the prepared entities
        // Hold the per-id locks across the read-modify-write window: concurrent
        // upsert-update calls for the same id must not race the snapshot read,
        // the merge, and the write.
        let _id_guards = self.acquire_id_locks(target, &prepared_ids).await;
        let existing_map = self.reader.get_batch(target, &prepared_ids).await?;

        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            let Some(existing) = existing_map.get(&entity.id) else {
                // Unconditional: an identifier that appeared since the read was
                // created by someone else, and this request has nothing to add
                // to it. Reported as neither a success nor a failure, which is
                // what this path has always done.
                writes.push(self.chunk_insert(target, entity, Precondition::None));
                continue;
            };

            let mut merged = existing.clone(); // clone: the update mutates its own copy of the stored payload
            let id = entity.id;
            // Upsert update mode applies Update Attributes semantics (clause
            // 5.6.8.4), the same whole-instance overwrite as a single update.
            if let Err(error) = update_entity_attrs(&mut merged, unwrap_expanded_ref(&entity.expanded)) {
                errors.push((id, error.into()));
                continue;
            }

            // Upsert-update mirrors the single attribute update (clause 5.6.9):
            // the per-attribute triggers, partitioned from the before/after diff.
            let groups = attribute_change_groups(existing, &merged);

            writes.push(self.chunk_update(
                target,
                id,
                UpdatedPayloads {
                    previous: existing,
                    merged: Arc::new(merged),
                    observed: Arc::new(entity.expanded),
                },
                &groups,
                Precondition::MustExist,
            ));
        }

        let outcome = self.commit_chunk(target, writes, mode).await?.settled();
        errors.extend(outcome.errors);

        Ok((
            BatchResult {
                success: outcome.success,
                errors: Vec::new(),
            },
            outcome.created,
        ))
    }
}

impl EntityService {
    /// The write that replaces `previous` with `payload`, tombstoning whatever
    /// the replacement drops (NGSI-LD 5.6.8).
    ///
    /// Mirrors the single replace (clause 5.6.18): one precise event for every
    /// created, updated, or deleted attribute instance.
    fn replacement_write(
        &self,
        target: &ContextTarget,
        id: Uri,
        payload: &Arc<Value>,
        previous: &Value,
        modified_at: DateTime<Utc>,
        _changed_attributes: Vec<AttributeName>,
    ) -> ChunkWrite {
        let groups = attribute_change_groups(previous, payload);
        self.chunk_update(
            target,
            id,
            UpdatedPayloads {
                previous,
                // clone: `Arc` ref-count bumps; the same payload is stored and observed.
                merged: Arc::clone(payload),
                observed: Arc::clone(payload),
            },
            &groups,
            Precondition::UnchangedSince(modified_at),
        )
    }
}
