//! Batch entity operations (clauses 5.6.7 to 5.6.10).
//!
//! Every one of them is N independent single-entity writes: the specification
//! returns per-entity results and explicitly permits partial success, so a
//! failure for one entity never rolls back another. Large batches are cut into
//! chunks so preparation and writing can overlap, and each chunk becomes one
//! commit whose statements the store fuses as it sees fit.
//!
//! An array is allowed to name the same entity more than once, and clause
//! 5.5.11 requires those instances to behave as if they had arrived in separate
//! requests, in array order. That ordering is imposed before any chunking, by
//! splitting the array into the successive rounds of
//! [`SameIdRounds`](crate::batch::same_id_rounds::SameIdRounds): a round holds
//! at most one instance per identifier, rounds run one after another, and only
//! the entries of a single round are chunked and committed concurrently. An
//! array without repeats is one round, so the ordinary batch is unaffected.
//!
//! Each operation reads the clock once, before its first round, and every
//! chunk of every round commits at that instant: "All Entities and Attributes
//! in the batch will get the same modifiedAt timestamp" (clause 5.5.11.0), the
//! instances of one repeated identifier included. An entity another write
//! changed while the batch ran, to a later instant than the batch's, is
//! written after that write instead, as a separate request would write it.

/// The entities of a batch write the admission let through.
pub mod admitted_batch;
/// Committing one chunk and settling what each entity in it did.
mod chunk;
/// Committing one chunk as a read-modify-write of each entity in it.
mod chunk_read_modify_write;
/// Compare-and-set tests for the batch read-modify-writes.
#[cfg(test)]
mod compare_and_set;
/// Preparing a batch whose entities name different contexts.
mod context_groups;
/// Batch entity creation.
mod create;
/// Batch entity deletion.
mod delete;
/// Durable-match recording tests for the batch operations.
#[cfg(test)]
mod durable_match_recording;
/// How much of a batch runs where.
pub mod execution;
/// History-recording tests for the batch operations.
#[cfg(test)]
mod history_recording;
/// Batch merge-patch.
mod merge;
/// Tests that a batch write reaching an entity another writer stamped later is
/// written after that write.
#[cfg(test)]
mod predated_writes;
/// Expanding and screening the entities of a batch that name one context.
mod prepare;
/// The whole state a write stores for one entity, across its attempts.
mod requested_state;
/// Ordering the instances an array repeats (clause 5.5.11).
mod same_id_rounds;
/// Tests that an entity whose fragment or replacement modifies nothing stores
/// no write.
#[cfg(test)]
mod unchanged_fragments;
/// Batch attribute update.
mod update;
/// Batch upsert, in both of its modes.
mod upsert;

use crate::{
    EntityService,
    batch::{
        admitted_batch::{AdmittedBatch, AdmittedBatchEntity},
        delete::DeleteChunkOutcome,
        same_id_rounds::SameIdRounds,
        upsert::UpsertChunkOutcome,
    },
    error::EntityError,
    side_effect::SideEffectPlacement,
    write_kind::{BatchCreate, BatchMerge, BatchUpdate, BatchUpsert},
};
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_expansion::written_member::WrittenMember;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use futures::{StreamExt, stream};
use std::collections::HashSet;

/// What a batch entity holds under an attribute name, as the batch moves it
/// between the tasks that prepare it: the members of an entity, or the
/// members of a merge fragment (clause 5.6.20).
pub trait BatchMember: WrittenMember + Send + Sync + 'static {}

impl<M: WrittenMember + Send + Sync + 'static> BatchMember for M {}

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
///
/// One submitted instance contributes exactly one entry, so an identifier the
/// array repeats appears once per instance, clause 5.5.11.0 has each instance
/// answered as if it had arrived in its own request.
pub struct BatchResult {
    /// Entity IDs that were successfully processed.
    pub success: Vec<Identifier>,
    /// Per-entity errors: `(entity_id, domain_error)` pairs.
    pub errors: Vec<(Identifier, EntityError)>,
}

impl BatchResult {
    /// An outcome that has processed nothing yet.
    const fn empty() -> BatchResult {
        BatchResult {
            success: Vec::new(),
            errors: Vec::new(),
        }
    }
}

impl EntityService {
    /// Whether a batch of `len` entities is cut into chunks of the configured size.
    const fn is_large_batch(&self, len: usize) -> bool {
        len >= self.batch_execution.large_threshold
    }

    /// Entities per chunk for a batch of `len`: the configured chunk size for a
    /// large batch, the whole batch otherwise, and never zero.
    fn batch_chunk_len(&self, len: usize) -> usize {
        if self.is_large_batch(len) {
            self.batch_execution.chunk_size.min(len).max(1)
        } else {
            len.max(1)
        }
    }

    /// The chunks one round is committed in, in round order.
    ///
    /// Committing these concurrently is safe exactly because they come from a
    /// round: a round holds each identifier at most once, so no two chunks can
    /// carry two instances of the same entity and land them out of array order
    /// (clause 5.5.11.0).
    fn round_chunks(&self, round: Vec<AdmittedBatchEntity>) -> Vec<Vec<AdmittedBatchEntity>> {
        let chunk_len = self.batch_chunk_len(round.len());
        let mut entries = round.into_iter();
        let mut chunks = Vec::new();
        loop {
            let chunk: Vec<AdmittedBatchEntity> = entries.by_ref().take(chunk_len).collect();
            if chunk.is_empty() {
                return chunks;
            }
            chunks.push(chunk);
        }
    }

    /// Create the admitted entities in a single batch operation (NGSI-LD 5.6.7).
    ///
    /// The batch writes the branch `placement` names and runs its side effects
    /// where `placement` puts them.
    ///
    /// # Errors
    ///
    /// Reports the first chunk-level failure: [`EntityError::Persistence`]
    /// when the store could not commit a chunk. A
    /// failure that concerns one entity only is carried in
    /// [`BatchResult::errors`] instead, because clause 5.6.7 reports batch
    /// results per entity.
    pub async fn batch_create(&self, placement: SideEffectPlacement<'_>, admitted: AdmittedBatch<BatchCreate>) -> Result<BatchResult, EntityError> {
        let timer = entity::start(EntityOperation::BatchCreate);
        let result: Result<BatchResult, EntityError> = async {
            let now = SystemInstant::new(Utc::now());
            let (_, entities) = admitted.into_parts();
            // Clause 5.5.11.1: only the first instance of an identifier creates, so
            // there is nothing left to order and no second round to run.
            let (deduped, errors) = Self::dedupe_create_entities(entities);
            let mut batch = BatchResult { success: Vec::new(), errors };
            if deduped.is_empty() {
                return Ok(batch);
            }

            let chunk_results: Vec<Result<BatchResult, EntityError>> = stream::iter(self.round_chunks(deduped))
                .map(|chunk| async move { self.batch_create_chunk(placement, chunk, now).await })
                .buffered(self.batch_execution.chunk_write_concurrency)
                .collect()
                .await;
            collect_chunks(&mut batch, chunk_results)?;

            Ok(batch)
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Delete multiple entities by ID in a single batch operation (NGSI-LD 5.6.10).
    ///
    /// The batch writes the branch `placement` names and runs its side effects
    /// where `placement` puts them.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store could not read or
    /// commit a chunk. An identifier that names no entity, or that the batch
    /// repeats, is carried in [`BatchResult::errors`] instead, because clause
    /// 5.6.10 reports batch results per entity.
    pub async fn batch_delete(&self, placement: SideEffectPlacement<'_>, entity_ids: &[Identifier]) -> Result<BatchResult, EntityError> {
        let timer = entity::start(EntityOperation::BatchDelete);
        let result: Result<BatchResult, EntityError> = async {
            let now = SystemInstant::new(Utc::now());
            // Clause 5.5.11.4: the first occurrence deletes, every later one is
            // answered as an entity that does not exist.
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
                .map(|chunk| async move { self.batch_delete_chunk(placement, chunk, now).await })
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
        .await;
        timer.finish_result(&result);
        result
    }

    /// Update attributes of the admitted entities in a single batch operation
    /// (NGSI-LD 5.6.9).
    ///
    /// The batch writes the branch `placement` names and runs its side effects
    /// where `placement` puts them.
    ///
    /// An identifier the array repeats is updated once per instance, in array
    /// order, each instance against the state its predecessor left: clause
    /// 5.5.11.3 has the broker send separate notifications for each individual
    /// update, which only holds if each is a separate write. Every instance
    /// still carries the one batch `modifiedAt` (clause 5.5.11.0).
    ///
    /// # Errors
    ///
    /// Reports the first chunk-level failure: [`EntityError::Persistence`]
    /// when the store could not read or commit a
    /// chunk. A failure that concerns one entity only is carried in
    /// [`BatchResult::errors`] instead, because clause 5.6.9 reports batch
    /// results per entity.
    pub async fn batch_update(
        &self,
        placement: SideEffectPlacement<'_>,
        admitted: AdmittedBatch<BatchUpdate>,
        overwrite: OverwritePolicy,
    ) -> Result<BatchResult, EntityError> {
        let timer = entity::start(EntityOperation::BatchUpdate);
        let result: Result<BatchResult, EntityError> = async {
            let now = SystemInstant::new(Utc::now());
            let (_, entities) = admitted.into_parts();
            let rounds = SameIdRounds::from_entries(entities, AdmittedBatchEntity::id);
            let mut batch = BatchResult::empty();
            for round in rounds.into_rounds() {
                let chunk_results: Vec<Result<BatchResult, EntityError>> = stream::iter(self.round_chunks(round))
                    .map(|chunk| async move { self.batch_update_chunk(placement, chunk, overwrite, now).await })
                    .buffered(self.batch_execution.chunk_write_concurrency)
                    .collect()
                    .await;
                collect_chunks(&mut batch, chunk_results)?;
            }
            Ok(batch)
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Merge the admitted partial updates into their entities in a single batch
    /// operation.
    ///
    /// The batch writes the branch `placement` names and runs its side effects
    /// where `placement` puts them.
    ///
    /// An identifier the array repeats is merged once per instance, in array
    /// order, each merge applied to what the previous one produced: clause
    /// 5.5.11.5 keeps only "the entity resulting from merging all of the entity
    /// instances, in the correct order" as the current state, and requires every
    /// instance to reach the Temporal Evolution of Entities.
    ///
    /// # Errors
    ///
    /// Reports the first chunk-level failure: [`EntityError::Persistence`]
    /// when the store could not read or commit a
    /// chunk. A failure that concerns one entity only is carried in
    /// [`BatchResult::errors`] instead.
    pub async fn batch_merge(&self, placement: SideEffectPlacement<'_>, admitted: AdmittedBatch<BatchMerge>) -> Result<BatchResult, EntityError> {
        let timer = entity::start(EntityOperation::BatchMerge);
        let result: Result<BatchResult, EntityError> = async {
            let now = SystemInstant::new(Utc::now());
            let (_, entities) = admitted.into_parts();
            let rounds = SameIdRounds::from_entries(entities, AdmittedBatchEntity::id);
            let mut batch = BatchResult::empty();
            for round in rounds.into_rounds() {
                let chunk_results: Vec<Result<BatchResult, EntityError>> = stream::iter(self.round_chunks(round))
                    .map(|chunk| async move { self.batch_merge_chunk(placement, chunk, now).await })
                    .buffered(self.batch_execution.chunk_write_concurrency)
                    .collect()
                    .await;
                collect_chunks(&mut batch, chunk_results)?;
            }
            Ok(batch)
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Create or update the admitted entities in a single batch operation
    /// (NGSI-LD 5.6.8), in the mode they were admitted for.
    ///
    /// The batch writes the branch `placement` names and runs its side effects
    /// where `placement` puts them.
    ///
    /// An identifier the array repeats is created by its first instance and
    /// replaced or updated by each later one, in array order, so the stored
    /// entity is the one clause 5.5.11.2 asks for: the result of merging all of
    /// the instances in order. Every instance is a write of its own, which is
    /// what puts all of them into the Temporal Evolution of Entities.
    ///
    /// # Errors
    ///
    /// Reports the first chunk-level failure: [`EntityError::Persistence`]
    /// when the store could not read or commit a chunk. A failure that concerns
    /// one entity only is carried in [`BatchResult::errors`] instead, because
    /// clause 5.6.8 reports batch results per entity, among them an entity
    /// the forwarding phase narrowed whose remainder does not expand, and
    /// [`EntityError::Contention`] for an entity whose write lost to a
    /// concurrent writer on every attempt.
    pub async fn batch_upsert(
        &self,
        placement: SideEffectPlacement<'_>,
        admitted: AdmittedBatch<BatchUpsert>,
    ) -> Result<(BatchResult, Vec<Identifier>), EntityError> {
        let timer = entity::start(EntityOperation::BatchUpsert);
        let result: Result<(BatchResult, Vec<Identifier>), EntityError> = async {
            let now = SystemInstant::new(Utc::now());
            let (BatchUpsert(policy), entities) = admitted.into_parts();
            let rounds = SameIdRounds::from_entries(entities, AdmittedBatchEntity::id);
            let mut batch = BatchResult::empty();
            let mut created = Vec::new();

            for round in rounds.into_rounds() {
                let chunk_results: Vec<Result<UpsertChunkOutcome, EntityError>> = stream::iter(self.round_chunks(round))
                    .map(|chunk| async move {
                        let mut chunk_errors = Vec::new();
                        let outcome = match policy {
                            UpsertPolicy::Update => self.batch_upsert_update_chunk(placement, chunk, &mut chunk_errors, now).await,
                            UpsertPolicy::Replace => self.batch_upsert_replace_chunk(placement, chunk, &mut chunk_errors, now).await,
                        };
                        outcome.map(|(result, chunk_created)| (result, chunk_errors, chunk_created))
                    })
                    .buffered(self.batch_execution.chunk_write_concurrency)
                    .collect()
                    .await;

                for result in chunk_results {
                    let (chunk_result, chunk_errors, mut chunk_created) = result?;
                    batch.success.extend(chunk_result.success);
                    batch.errors.extend(chunk_errors);
                    batch.errors.extend(chunk_result.errors);
                    created.append(&mut chunk_created);
                }
            }

            Ok((batch, created))
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

/// Fold every chunk's result into `batch`, failing on the first chunk-level
/// error. A per-entity failure is carried in `errors` and does not fail here.
fn collect_chunks(batch: &mut BatchResult, chunk_results: Vec<Result<BatchResult, EntityError>>) -> Result<(), EntityError> {
    for result in chunk_results {
        let chunk_result = result?;
        batch.success.extend(chunk_result.success);
        batch.errors.extend(chunk_result.errors);
    }
    Ok(())
}
