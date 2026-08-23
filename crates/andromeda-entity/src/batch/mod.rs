//! Batch entity operations (clauses 5.6.7 to 5.6.10).
//!
//! Every one of them is N independent single-entity writes: the specification
//! returns per-entity results and explicitly permits partial success, so a
//! failure for one entity never rolls back another. Large batches are cut into
//! chunks so preparation and writing can overlap, and each chunk becomes one
//! commit whose statements the store fuses as it sees fit.

/// Committing one chunk and settling what each entity in it did.
mod chunk;
/// Batch entity creation.
mod create;
/// Batch entity deletion.
mod delete;
/// Durable-match recording tests for the batch operations.
#[cfg(test)]
mod durable_match_recording;
/// How much of a batch runs where.
pub mod execution;
/// Batch merge-patch.
mod merge;
/// Turning request entities into expanded, validated payloads.
mod prepare;
/// Batch attribute update.
mod update;
/// Batch upsert, in both of its modes.
mod upsert;

use crate::{EntityService, batch::delete::DeleteChunkOutcome, error::EntityError, side_effect::BatchSideEffectMode};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{entity::Entity, expanded::mutation::selection::OverwritePolicy};
use futures::{StreamExt, stream};
use std::collections::HashSet;

/// How an upsert treats attributes already held by an existing entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpsertPolicy {
    /// Merge submitted attributes into the existing entity.
    Update,
    /// Replace the existing entity representation.
    Replace,
}

/// Domain-level result of a batch operation.
///
/// Contains the IDs of successfully processed entities and the per-entity
/// errors for those that failed. Converted to HTTP-layer
/// [`BatchOperationResult`](andromeda_ngsi_ld_problem_details::batch_operation::BatchOperationResult)
/// at the handler boundary.
pub struct BatchResult {
    /// Entity IDs that were successfully processed.
    pub success: Vec<Uri>,
    /// Per-entity errors: `(entity_id, domain_error)` pairs.
    pub errors: Vec<(Uri, EntityError)>,
}

impl EntityService {
    const fn is_large_batch(&self, len: usize) -> bool {
        len >= self.batch_execution.large_threshold
    }

    fn batch_chunk_len(&self, len: usize) -> usize {
        if self.is_large_batch(len) {
            self.batch_execution.chunk_size.min(len).max(1)
        } else {
            len.max(1)
        }
    }

    /// Create multiple entities in a single batch operation (NGSI-LD 5.6.7).
    pub async fn batch_create(
        &self,
        target: &ContextTarget,
        entities: &[Entity],
        context: &ContextSpec,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, EntityError> {
        self.batch_activity.record_activity();
        let (deduped, mut errors) = Self::dedupe_create_entities(entities);
        if deduped.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let chunk_len = self.batch_chunk_len(deduped.len());
        let chunk_results: Vec<Result<BatchResult, EntityError>> = stream::iter(deduped.chunks(chunk_len))
            .map(|chunk| async move { self.batch_create_chunk(target, chunk, context, mode).await })
            .buffered(self.batch_execution.chunk_write_concurrency)
            .collect()
            .await;

        let mut success = Vec::new();
        for result in chunk_results {
            let chunk_result = result?;
            success.extend(chunk_result.success);
            errors.extend(chunk_result.errors);
        }

        Ok(BatchResult { success, errors })
    }

    /// Delete multiple entities by ID in a single batch operation (NGSI-LD 5.6.10).
    pub async fn batch_delete(&self, target: &ContextTarget, entity_ids: &[Uri], mode: BatchSideEffectMode) -> Result<BatchResult, EntityError> {
        self.batch_activity.record_activity();
        let mut seen = HashSet::new();
        let mut unique_ids = Vec::new();
        let mut errors = Vec::new();
        for id in entity_ids {
            if seen.insert(id.as_str()) {
                unique_ids.push(id.clone()); // clone: the unique batch owns its identifiers
            } else {
                errors.push((id.clone(), EntityError::not_found(id))); // clone: reported alongside the error
            }
        }

        let chunk_len = self.batch_chunk_len(unique_ids.len());
        let chunk_results: Vec<Result<DeleteChunkOutcome, EntityError>> = stream::iter(unique_ids.chunks(chunk_len))
            .map(|chunk| async move { self.batch_delete_chunk(target, chunk, mode).await })
            .buffered(self.batch_execution.chunk_write_concurrency)
            .collect()
            .await;

        let mut success = Vec::new();
        for result in chunk_results {
            let (chunk_success, chunk_errors) = result?;
            success.extend(chunk_success);
            errors.extend(chunk_errors);
        }

        Ok(BatchResult { success, errors })
    }

    /// Update attributes of multiple entities in a single batch operation (NGSI-LD 5.6.9).
    pub async fn batch_update(
        &self,
        target: &ContextTarget,
        entities: &[Entity],
        context: &ContextSpec,
        overwrite: OverwritePolicy,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, EntityError> {
        self.batch_activity.record_activity();
        let chunk_len = self.batch_chunk_len(entities.len());
        let chunk_results: Vec<Result<BatchResult, EntityError>> = stream::iter(entities.chunks(chunk_len))
            .map(|chunk| async move { self.batch_update_chunk(target, chunk, context, overwrite, mode).await })
            .buffered(self.batch_execution.chunk_write_concurrency)
            .collect()
            .await;

        collect_chunks(chunk_results)
    }

    /// Merge partial updates into multiple entities in a single batch operation.
    pub async fn batch_merge(
        &self,
        target: &ContextTarget,
        entities: &[Entity],
        context: &ContextSpec,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, EntityError> {
        self.batch_activity.record_activity();
        let chunk_len = self.batch_chunk_len(entities.len());
        let chunk_results: Vec<Result<BatchResult, EntityError>> = stream::iter(entities.chunks(chunk_len))
            .map(|chunk| async move { self.batch_merge_chunk(target, chunk, context, mode).await })
            .buffered(self.batch_execution.chunk_write_concurrency)
            .collect()
            .await;

        collect_chunks(chunk_results)
    }

    /// Create or update multiple entities in a single batch operation (NGSI-LD 5.6.8).
    pub async fn batch_upsert(
        &self,
        target: &ContextTarget,
        entities: &[Entity],
        context: &ContextSpec,
        policy: UpsertPolicy,
        mode: BatchSideEffectMode,
    ) -> Result<(BatchResult, Vec<Uri>), EntityError> {
        self.batch_activity.record_activity();
        let deduped = Self::dedupe_upsert_entities(entities);
        let chunk_len = self.batch_chunk_len(deduped.len());

        type ChunkOutcome = (BatchResult, Vec<(Uri, EntityError)>, Vec<Uri>);
        let chunk_results: Vec<Result<ChunkOutcome, EntityError>> = stream::iter(deduped.chunks(chunk_len))
            .map(|chunk| async move {
                let mut chunk_errors = Vec::new();
                let outcome = match policy {
                    UpsertPolicy::Update => self.batch_upsert_update_chunk(target, chunk, context, &mut chunk_errors, mode).await,
                    UpsertPolicy::Replace => self.batch_upsert_replace_chunk(target, chunk, context, &mut chunk_errors, mode).await,
                };
                outcome.map(|(result, created)| (result, chunk_errors, created))
            })
            .buffered(self.batch_execution.chunk_write_concurrency)
            .collect()
            .await;

        let mut errors = Vec::new();
        let mut success = Vec::new();
        let mut created = Vec::new();
        for result in chunk_results {
            let (chunk_result, chunk_errors, mut chunk_created) = result?;
            success.extend(chunk_result.success);
            errors.extend(chunk_errors);
            errors.extend(chunk_result.errors);
            created.append(&mut chunk_created);
        }

        Ok((BatchResult { success, errors }, created))
    }
}

/// Fold every chunk's result into the batch's, failing on the first chunk-level
/// error. A per-entity failure is carried in `errors` and does not fail here.
fn collect_chunks(chunk_results: Vec<Result<BatchResult, EntityError>>) -> Result<BatchResult, EntityError> {
    let mut success = Vec::new();
    let mut errors = Vec::new();
    for result in chunk_results {
        let chunk_result = result?;
        success.extend(chunk_result.success);
        errors.extend(chunk_result.errors);
    }
    Ok(BatchResult { success, errors })
}
