//! Batch upsert (clause 5.6.8), in both of its modes.
//!
//! Replace mode overwrites `entities.payload` wholesale, so any attribute the
//! replacement omits is deleted and needs a tombstone. That diff is only
//! meaningful against the payload the entity actually had when the write
//! landed, so each replacement states the `modifiedAt` its diff assumed and is
//! recomputed against a fresh read if a concurrent writer got in first. The
//! diff is taken against `entities.payload` rather than the temporal catalogue
//! on purpose: catalogue rows outlive the attributes they describe, because a
//! deletion writes a tombstone rather than dropping the row, so diffing against
//! them would re-tombstone already-deleted attributes on every upsert.
//!
//! Update mode merges instead of replacing, so nothing is dropped and no
//! tombstone is implied.

use crate::{
    error::AndromedaError,
    service::entity::{
        EntityService,
        batch::{BatchResult, chunk::ChunkWrite},
        expanded_member::extract_attribute_keys,
        history_record::HistoryRecord,
        precondition::{Precondition, WRITE_ATTEMPTS},
        side_effect::BatchSideEffectMode,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{instance_key::removed_attribute_instances, mutation::entity::merge_entity},
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// One chunk's per-entity upsert results.
type UpsertOutcome = (BatchResult, Vec<String>);

impl EntityService {
    /// Keep only the last appearance of each identifier, which is the one a
    /// later write would have left behind anyway.
    pub(super) fn dedupe_upsert_entities(entities: &[Entity]) -> Vec<&Entity> {
        let mut seen = HashSet::with_capacity(entities.len());
        entities
            .iter()
            .rev()
            .filter(|entity| seen.insert(entity.id.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub(super) async fn batch_upsert_replace_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(String, AndromedaError)>,
        mode: BatchSideEffectMode,
    ) -> Result<UpsertOutcome, AndromedaError> {
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
        let lock_ids: Vec<String> = chunk_ids.iter().map(Uri::to_string).collect(); // the id-lock registry is keyed by text
        let _id_guards = self.acquire_id_locks(&lock_ids).await;

        let snapshots = self.repo.get_snapshot_batch(&chunk_ids).await?;
        let mut payloads: HashMap<Uri, Arc<Value>> = HashMap::with_capacity(prepared.len());
        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            let write = match snapshots.get(&entity.id) {
                Some(snapshot) => {
                    let payload = Arc::new(entity.expanded);
                    replacement_write(entity.id, &payload, &snapshot.payload, snapshot.modified_at, entity.changed_attributes)
                }
                None => ChunkWrite::insert(entity, Precondition::MustNotExist),
            };
            // clone: an `Arc` handle kept for a possible retry, which must not
            // re-expand the request payload.
            payloads.insert(write.id.clone(), Arc::clone(&write.observed));
            writes.push(write);
        }

        let mut success = Vec::new();
        let mut created = Vec::new();
        let mut outcome = self.commit_chunk(writes, mode).await?;

        for attempt in 1..=WRITE_ATTEMPTS {
            success.extend(std::mem::take(&mut outcome.success));
            created.extend(std::mem::take(&mut outcome.created));
            errors.extend(std::mem::take(&mut outcome.errors));

            let retry: Vec<(Uri, AndromedaError)> = std::mem::take(&mut outcome.retryable);
            if retry.is_empty() {
                break;
            }
            if attempt == WRITE_ATTEMPTS {
                errors.extend(retry.into_iter().map(|(id, error)| (id.to_string(), error)));
                break;
            }

            let retry_ids: Vec<Uri> = retry.into_iter().map(|(id, _error)| id).collect();
            let snapshots = self.repo.get_snapshot_batch(&retry_ids).await?;
            let mut retry_writes = Vec::with_capacity(retry_ids.len());
            for id in retry_ids {
                let Some(payload) = payloads.get(&id) else {
                    continue;
                };
                let changed = extract_attribute_keys(payload);
                // clone: `Arc` ref-count bumps; the payload is reused for every
                // attempt this entity needs.
                let write = match snapshots.get(&id) {
                    Some(snapshot) => replacement_write(id, payload, &snapshot.payload, snapshot.modified_at, changed),
                    None => ChunkWrite::insert_payload(id, Arc::clone(payload), changed, Precondition::MustNotExist),
                };
                retry_writes.push(write);
            }
            outcome = self.commit_chunk(retry_writes, mode).await?;
        }

        Ok((BatchResult { success, errors: Vec::new() }, created))
    }

    pub(super) async fn batch_upsert_update_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(String, AndromedaError)>,
        mode: BatchSideEffectMode,
    ) -> Result<UpsertOutcome, AndromedaError> {
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
        let lock_ids: Vec<String> = prepared_ids.iter().map(Uri::to_string).collect(); // the id-lock registry is keyed by text
        // Hold the per-id locks across the read-modify-write window: concurrent
        // upsert-update calls for the same id must not race the snapshot read,
        // the merge, and the write.
        let _id_guards = self.acquire_id_locks(&lock_ids).await;
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            let Some(existing) = existing_map.get(&entity.id) else {
                // Unconditional: an identifier that appeared since the read was
                // created by someone else, and this request has nothing to add
                // to it. Reported as neither a success nor a failure, which is
                // what this path has always done.
                writes.push(ChunkWrite::insert(entity, Precondition::None));
                continue;
            };

            let mut merged = existing.clone(); // clone: the merge mutates its own copy of the stored payload
            let id = entity.id;
            if let Err(error) = merge_entity(&mut merged, &entity.expanded) {
                errors.push((id.to_string(), error.into()));
                continue;
            }

            writes.push(ChunkWrite::update(
                id,
                Arc::new(merged),
                Arc::new(entity.expanded),
                entity.changed_attributes,
                Precondition::MustExist,
            ));
        }

        let outcome = self.commit_chunk(writes, mode).await?.settled();
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

/// The write that replaces `previous` with `payload`, tombstoning whatever the
/// replacement drops (NGSI-LD 5.6.8).
fn replacement_write(id: Uri, payload: &Arc<Value>, previous: &Value, modified_at: DateTime<Utc>, changed_attributes: Vec<AttributeName>) -> ChunkWrite {
    // clone: `Arc` ref-count bumps; the same payload is stored and observed.
    let mut write = ChunkWrite::update(
        id,
        Arc::clone(payload),
        Arc::clone(payload),
        changed_attributes,
        Precondition::UnchangedSince(modified_at),
    );
    write.tombstones = removed_attribute_instances(previous, payload)
        .into_iter()
        .map(HistoryRecord::Tombstone)
        .collect();
    write
}
