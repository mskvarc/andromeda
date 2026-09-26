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
//! removals imply tombstones. Clause 5.6.8.4 is what puts them there: its
//! update mode runs "the behaviour defined by clause 5.6.2 [...] limited to a
//! local operation", and clause 5.6.2.4 is one of the three clauses that say
//! "NGSI-LD Nulls should be supported by this operation".
//!
//! Neither mode sees an identifier twice: clause 5.5.11.2 requires the
//! instances an array repeats to be replaced or updated "in chronological
//! order", so the coordinator hands each one to a separate round, and a chunk
//! carries at most one instance of any entity.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        UpsertPolicy,
        chunk::{ChunkWrite, UpdatedPayloads},
        prepare::PreparedEntity,
    },
    error::EntityError,
    expanded_member::{unwrap_expanded_ref, written_attribute_names},
    notification_plan::attribute_change_groups,
    side_effect::BatchSideEffectMode,
    write_admission::BatchWrite,
};
use andromeda_context_commit::{precondition::Precondition, write_retry::WRITE_ATTEMPTS};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{
        mutation::{
            deletion_sentinel::{DroppedSentinels, drop_deletion_sentinels},
            entity::update_entity_attrs,
            selection::OverwritePolicy,
        },
        written_observation::AddressedInstances,
    },
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
        // Clause 5.6.8.4 gives replace mode two branches and neither admits the
        // sentinel: an Entity that does not exist is created by "the behaviour
        // defined by clause 5.6.1", and one that does "shall be completely
        // replaced by the new Entity content provided", which is the Replace
        // Entity of clause 5.6.18 -- whose clause 5.6.18.4 states outright that
        // NGSI-LD Nulls are not supported. Clause 5.5.4 excepts only partial
        // update and merge fragments, and a replacement is neither.
        let (prepared, mut chunk_errors) = self
            .prepare_entity_refs(entities, context, BatchWrite::Upsert(UpsertPolicy::Replace).null_sentinel_admission())
            .await;
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
        let now = Utc::now();

        let snapshots = self.reader.get_snapshot_batch(target, &chunk_ids).await?;
        let mut payloads: HashMap<Uri, Arc<Value>> = HashMap::with_capacity(prepared.len());
        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            let write = match snapshots.get(&entity.id) {
                Some(snapshot) => {
                    let payload = Arc::new(entity.expanded);
                    self.replacement_write(target, entity.id, &payload, &snapshot.payload, snapshot.modified_at, now)
                }
                None => self.chunk_insert(target, entity, Precondition::MustNotExist, now),
            };
            match write {
                Ok(write) => {
                    // An `Arc` handle to the payload the write stores is kept for a
                    // possible retry, which must not re-expand the request payload.
                    if let Some(record) = write.entity.record() {
                        payloads.insert(write.id.clone(), Arc::clone(&record.payload)); // clone: the retry map owns its key and a shared payload handle.
                    }
                    writes.push(write);
                }
                Err(failure) => errors.push(*failure),
            }
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
                let changed = match written_attribute_names(payload) {
                    Ok(changed) => changed,
                    Err(error) => {
                        errors.push((id, error));
                        continue;
                    }
                };
                // clone: `Arc` ref-count bumps; the payload is reused for every
                // attempt this entity needs.
                let write = match snapshots.get(&id) {
                    Some(snapshot) => self.replacement_write(target, id, payload, &snapshot.payload, snapshot.modified_at, now),
                    None => self.chunk_insert_payload(target, id, Arc::clone(payload), changed, Precondition::MustNotExist, now),
                };
                match write {
                    Ok(write) => retry_writes.push(write),
                    Err(failure) => errors.push(*failure),
                }
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
        // Clause 5.6.8.4: "If there were an existing Entity with the same Entity
        // ID, the behaviour defined by clause 5.6.2 shall be executed, but
        // limited to a local operation, if the requested update mode is
        // 'update'." Clause 5.6.2.4 is one of the three clauses carrying "NGSI-LD
        // Nulls should be supported by this operation", so this mode takes the
        // partial update fragment's admission and not the create branch's.
        //
        // Whether an Entity exists is not known while the payload is screened,
        // and the same payload creates it when it does not. The screening
        // therefore states the update branch's admission, and the create branch
        // drops the deletions rather than storing them: clause 5.5.8 deletes
        // "each member of the Fragment, whose value is an NGSI-LD Null,
        // contained by the target", and a target that does not exist contains
        // none of them.
        let (prepared, mut chunk_errors) = self
            .prepare_entity_refs(entities, context, BatchWrite::Upsert(UpsertPolicy::Update).null_sentinel_admission())
            .await;
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
        let now = Utc::now();

        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            let Some(existing) = existing_map.get(&entity.id) else {
                // Unconditional: an identifier that appeared since the read was
                // created by someone else, and this request has nothing to add
                // to it. Reported as neither a success nor a failure, which is
                // what this path has always done.
                match self.chunk_insert_created(target, entity, now) {
                    Ok(write) => writes.push(write),
                    Err(failure) => errors.push(*failure),
                }
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
            let groups = match attribute_change_groups(existing, &merged) {
                Ok(groups) => groups,
                Err(error) => {
                    errors.push((id, error));
                    continue;
                }
            };

            match self.chunk_update(
                target,
                id,
                UpdatedPayloads {
                    previous: existing,
                    merged: Arc::new(merged),
                    addressed: AddressedInstances::EntityFragment {
                        fragment: &entity.expanded,
                        overwrite: OverwritePolicy::Overwrite,
                    },
                },
                &groups,
                Precondition::MustExist,
                now,
            ) {
                Ok(write) => writes.push(write),
                Err(failure) => errors.push(*failure),
            }
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
    /// The write that creates an entity the upsert's update mode found absent.
    ///
    /// Clause 5.6.8.4 routes it to "the behaviour defined by clause 5.6.1",
    /// where the deletions the update branch admits have nothing to delete:
    /// they are dropped, and only a payload that carried one is re-derived,
    /// so an ordinary create keeps the columns the preparation pass computed.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report, when the payload's members cannot be read.
    fn chunk_insert_created(&self, target: &ContextTarget, prepared: PreparedEntity, now: DateTime<Utc>) -> Result<ChunkWrite, Box<(Uri, EntityError)>> {
        let mut prepared = prepared;
        match drop_deletion_sentinels(&mut prepared.expanded) {
            DroppedSentinels::Unchanged => self.chunk_insert(target, prepared, Precondition::None, now),
            DroppedSentinels::Dropped => {
                let payload = Arc::new(prepared.expanded);
                match written_attribute_names(&payload) {
                    Ok(changed_attributes) => self.chunk_insert_payload(target, prepared.id, payload, changed_attributes, Precondition::None, now),
                    Err(error) => Err(Box::new((prepared.id, error))),
                }
            }
        }
    }

    /// The write that replaces `previous` with `payload`, tombstoning whatever
    /// the replacement drops (NGSI-LD 5.6.8).
    ///
    /// Mirrors the single replace (clause 5.6.18): one precise event for every
    /// created, updated, or deleted attribute instance.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report, when either state's members cannot be read.
    fn replacement_write(
        &self,
        target: &ContextTarget,
        id: Uri,
        payload: &Arc<Value>,
        previous: &Value,
        modified_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ChunkWrite, Box<(Uri, EntityError)>> {
        let groups = match attribute_change_groups(previous, payload) {
            Ok(groups) => groups,
            Err(error) => return Err(Box::new((id, error))),
        };
        self.chunk_update(
            target,
            id,
            UpdatedPayloads {
                previous,
                merged: Arc::clone(payload), // clone: Arc ref-count bump; the payload is reused on a retry.
                addressed: AddressedInstances::EntityFragment {
                    fragment: payload,
                    overwrite: OverwritePolicy::Overwrite,
                },
            },
            &groups,
            Precondition::UnchangedSince(modified_at),
            now,
        )
    }
}
