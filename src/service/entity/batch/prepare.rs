//! Turning a batch of request entities into expanded, validated payloads.
//!
//! Entities whose `@context` matches the batch's resolved handle take a
//! synchronous fast path; the rest are expanded through the JSON-LD engine,
//! in bulk where they share the handle and one at a time where they do not.
//! Every entry carries its input index so per-entity errors line up with the
//! caller's order regardless of which pool produced them.

use crate::{
    error::AndromedaError,
    service::entity::{
        EntityService,
        attribute_envelope::validate_attribute_envelopes,
        batch::execution::{BatchExecution, decide_batch_execution},
        expanded_member::extract_attribute_keys,
    },
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{batch_context::BatchContextHandle, processor::JsonLdEngine},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::grammar::ScopeBuf, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{
        entity_member::{extract_expires_at, extract_scopes, extract_types},
        geo_property::{extract_location_geometry, extract_observation_space_geometry, extract_operation_space_geometry},
    },
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinSet};

/// Pre-processed entity data ready for the write path.
///
/// Holds every field extracted during preparation — serialization,
/// normalization, JSON-LD expansion, validation — so the write path never has
/// to walk the payload again.
pub(super) struct PreparedEntity {
    /// Entity identifier.
    pub id: Uri,
    /// Expanded JSON-LD payload (the canonical stored form).
    pub expanded: Value,
    /// Entity type names extracted from the expanded payload.
    pub types: Vec<EntityTypeName>,
    /// Scope values extracted from the expanded payload.
    pub scopes: Vec<ScopeBuf>,
    /// Attribute names changed by this request payload.
    pub changed_attributes: Vec<AttributeName>,
    /// The `location` geometry, if present.
    pub location: Option<Geometry>,
    /// The `observationSpace` geometry, if present.
    pub observation_space: Option<Geometry>,
    /// The `operationSpace` geometry, if present.
    pub operation_space: Option<Geometry>,
    /// Entity expiration timestamp (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
}

/// One entity entered into the batch classifier. Carries the input index
/// so per-entity results can be reassembled into the caller's order
/// regardless of which pool processed the entry.
pub(super) struct ClassifiedEntry {
    pub(super) idx: usize,
    pub(super) id: Uri,
    pub(super) value: Value,
}

/// Result of [`classify_batch`]: input entries split into the
/// synchronous fastpath bucket and the asynchronous slow-path bucket.
pub(super) struct ClassifiedBatch {
    pub(super) fast: Vec<ClassifiedEntry>,
    pub(super) slow: Vec<ClassifiedEntry>,
}

/// `(input_index, prepared_entity_or_error)` tuple emitted by either pool.
pub(super) type PreparedSlot = Result<PreparedEntity, (String, AndromedaError)>;
type IndexedPrepared = (usize, PreparedSlot);

/// Output of one entry processed by the fastpath chunk worker.
enum FastPoolOutput {
    /// Fastpath produced a result (success or per-entity failure).
    ///
    /// Boxed because a [`PreparedEntity`] is an order of magnitude larger than
    /// a [`ClassifiedEntry`], and these are collected into one `Vec` per chunk;
    /// inlining it would size every element for the largest variant.
    Done { idx: usize, result: Box<PreparedSlot> },
    /// Fastpath bailed at runtime (e.g. unsupported attribute shape);
    /// the entry must be retried on the slow path.
    Demoted(ClassifiedEntry),
}

/// Split entries by whether the fast path can expand them.
pub(super) fn classify_batch(entries: Vec<ClassifiedEntry>, handle: &BatchContextHandle, engine: &JsonLdEngine) -> ClassifiedBatch {
    let mut fast = Vec::new();
    let mut slow = Vec::new();
    let handle_eligible = handle.fast_eligible();
    for entry in entries {
        if handle_eligible && engine.entity_context_matches_handle(&entry.value, handle) {
            fast.push(entry);
        } else {
            slow.push(entry);
        }
    }
    ClassifiedBatch { fast, slow }
}

fn process_fast_entry(entry: ClassifiedEntry, engine: &JsonLdEngine, handle: &BatchContextHandle) -> FastPoolOutput {
    match engine.try_fast_expand(&entry.value, handle) {
        Some(expanded) => FastPoolOutput::Done {
            idx: entry.idx,
            result: Box::new(EntityService::build_prepared_entity(entry.id, expanded)),
        },
        None => FastPoolOutput::Demoted(entry),
    }
}

impl EntityService {
    pub(super) fn batch_execution(&self, len: usize, fast_count: usize, slow_count: usize) -> BatchExecution {
        decide_batch_execution(
            len,
            fast_count,
            slow_count,
            self.batch_prepare_blocking_threshold,
            self.batch_chunk_concurrency,
            self.batch_concurrency,
        )
    }

    fn build_prepared_entity(id: Uri, expanded: Value) -> PreparedSlot {
        validate_attribute_envelopes(&expanded).map_err(|err| (id.to_string(), err))?;
        let changed_attributes = extract_attribute_keys(&expanded);
        let types = extract_types(&expanded);
        let scopes = extract_scopes(&expanded);
        let location = extract_location_geometry(&expanded);
        let observation_space = extract_observation_space_geometry(&expanded);
        let operation_space = extract_operation_space_geometry(&expanded);
        let expires_at = extract_expires_at(&expanded);

        Ok(PreparedEntity {
            id,
            expanded,
            types,
            scopes,
            changed_attributes,
            location,
            observation_space,
            operation_space,
            expires_at,
        })
    }

    async fn run_fast_pool(&self, fast: Vec<ClassifiedEntry>, handle: &BatchContextHandle, execution: BatchExecution) -> Vec<FastPoolOutput> {
        if fast.is_empty() {
            return Vec::new();
        }
        match execution {
            BatchExecution::Inline => fast.into_iter().map(|entry| process_fast_entry(entry, &self.engine, handle)).collect(),
            BatchExecution::Offload { fast_chunks, .. } => {
                // Distribute the fastpath work across blocking workers in
                // chunks of `batch_chunk_size` entries each, capping
                // concurrent in-flight chunks to `fast_chunks` via a
                // semaphore so we do not flood the Tokio blocking pool.
                let chunk_size = self.batch_chunk_size.max(1);
                let semaphore = Arc::new(Semaphore::new(fast_chunks.get()));
                let mut joinset: JoinSet<Vec<FastPoolOutput>> = JoinSet::new();
                let mut chunk: Vec<ClassifiedEntry> = Vec::with_capacity(chunk_size);
                let spawn_chunk = async |owned: Vec<ClassifiedEntry>, joinset: &mut JoinSet<Vec<FastPoolOutput>>| -> Result<(), AndromedaError> {
                    // Semaphore is closed only when the Arc is dropped; the
                    // owning Arc is held by `semaphore` and the inner closure,
                    // so a closed error here is unreachable. Treat it as an
                    // internal error to keep the function panic-free.
                    let permit = Arc::clone(&semaphore)
                        .acquire_owned()
                        .await
                        .map_err(|err| AndromedaError::internal(format!("fastpath semaphore closed: {err}")))?;
                    let engine = self.engine.clone(); // clone: Arc-shared engine internals
                    let handle_local = handle.clone(); // clone: Arc-shared resolved context
                    joinset.spawn_blocking(move || {
                        let _permit = permit;
                        owned.into_iter().map(|entry| process_fast_entry(entry, &engine, &handle_local)).collect()
                    });
                    Ok(())
                };
                let mut spawn_errors: Vec<(String, AndromedaError)> = Vec::new();
                for entry in fast {
                    chunk.push(entry);
                    if chunk.len() == chunk_size {
                        let owned = std::mem::replace(&mut chunk, Vec::with_capacity(chunk_size));
                        if let Err(err) = spawn_chunk(owned, &mut joinset).await {
                            spawn_errors.push(("unknown".to_string(), err));
                        }
                    }
                }
                if !chunk.is_empty()
                    && let Err(err) = spawn_chunk(chunk, &mut joinset).await
                {
                    spawn_errors.push(("unknown".to_string(), err));
                }

                let mut outputs: Vec<FastPoolOutput> = Vec::new();
                for (id, error) in spawn_errors {
                    outputs.push(FastPoolOutput::Done {
                        idx: usize::MAX,
                        result: Box::new(Err((id, error))),
                    });
                }
                while let Some(result) = joinset.join_next().await {
                    match result {
                        Ok(chunk_outputs) => outputs.extend(chunk_outputs),
                        Err(error) => {
                            outputs.push(FastPoolOutput::Done {
                                idx: usize::MAX,
                                result: Box::new(Err((
                                    "unknown".to_string(),
                                    AndromedaError::internal(format!("Batch preparation task failed: {error}")),
                                ))),
                            });
                        }
                    }
                }
                outputs
            }
        }
    }

    async fn run_slow_pool(&self, slow: Vec<ClassifiedEntry>, handle: &BatchContextHandle, execution: BatchExecution) -> Vec<IndexedPrepared> {
        if slow.is_empty() {
            return Vec::new();
        }

        // Pre-process every slow entry: NGSI-LD normalize then apply the
        // batch handle's `@context`. Entries whose inline `@context`
        // diverges from the handle keep their own context and require
        // per-entity expansion; the rest can ride a single bulk expand call.
        let mut hit_pending: Vec<(usize, Uri, Value)> = Vec::new();
        let mut miss_pending: Vec<(usize, Uri, Value)> = Vec::new();
        for ClassifiedEntry { idx, id, mut value } in slow {
            normalize_entity_attributes(&mut value);
            match self.engine.entity_context_matches_handle(&value, handle) {
                true => {
                    if let Some(obj) = value.as_object_mut()
                        && !obj.contains_key("@context")
                        && let Some(resolved) = handle.spec().to_context_value()
                    {
                        obj.insert("@context".to_string(), resolved);
                    }
                    hit_pending.push((idx, id, value));
                }
                false => miss_pending.push((idx, id, value)),
            }
        }

        let mut results: Vec<IndexedPrepared> = Vec::new();

        if !hit_pending.is_empty() {
            let docs: Vec<Value> = hit_pending.iter().map(|(_, _, v)| v.clone()).collect(); // clone: bulk path needs originals for retry
            let bulk = self.engine.expand_prepared_array(docs, handle.spec().core_policy()).await;
            for ((idx, id, _), expanded) in hit_pending.into_iter().zip(bulk) {
                let result = match expanded {
                    Ok(v) => Self::build_prepared_entity(id, v),
                    Err(err) => Err((id.to_string(), err.into())),
                };
                results.push((idx, result));
            }
        }

        // Entries whose `@context` diverges still need per-entity expand.
        if !miss_pending.is_empty() {
            let concurrency = match execution {
                BatchExecution::Inline => 1,
                BatchExecution::Offload { slow_concurrency, .. } => slow_concurrency.get(),
            };
            let miss_results: Vec<IndexedPrepared> = stream::iter(miss_pending)
                .map(|(idx, id, value)| async move {
                    let expanded = match self.engine.expand(value, handle.spec()).await {
                        Ok(v) => v,
                        Err(err) => return (idx, Err((id.to_string(), err.into()))),
                    };
                    (idx, Self::build_prepared_entity(id, expanded))
                })
                .buffered(concurrency)
                .collect()
                .await;
            results.extend(miss_results);
        }

        results
    }

    pub(super) async fn prepare_entity_refs(&self, entities: &[&Entity], context: &ContextSpec) -> (Vec<PreparedEntity>, Vec<(String, AndromedaError)>) {
        let len = entities.len();
        let handle = self.engine.batch_context_handle(context).await;

        // Serialize all entities to JSON upfront so per-entity serialization
        // failures are reported and the rest of the batch can proceed.
        let mut entries: Vec<ClassifiedEntry> = Vec::with_capacity(len);
        let mut errors: Vec<(String, AndromedaError)> = Vec::new();
        for (idx, entity) in entities.iter().enumerate() {
            let id = entity.id.clone(); // clone: owned identifier for the classified entry
            match serde_json::to_value(*entity) {
                Ok(value) => entries.push(ClassifiedEntry { idx, id, value }),
                Err(err) => errors.push((id.to_string(), AndromedaError::entity_validation(format!("Invalid entity payload: {}", err)))),
            }
        }

        let classified = classify_batch(entries, &handle, &self.engine);
        let execution = self.batch_execution(len, classified.fast.len(), classified.slow.len());

        // Process the fast bucket; collect demotions for the slow pool.
        let fast_outputs = self.run_fast_pool(classified.fast, &handle, execution).await;
        let mut indexed_results: Vec<Option<PreparedSlot>> = (0..len).map(|_| None).collect();
        let mut demoted: Vec<ClassifiedEntry> = Vec::new();
        for output in fast_outputs {
            match output {
                FastPoolOutput::Done { idx, result } => {
                    if idx < len {
                        indexed_results[idx] = Some(*result);
                    } else {
                        // Pool-level error (e.g. JoinError) — record it
                        // without an index slot.
                        if let Err(err) = *result {
                            errors.push(err);
                        }
                    }
                }
                FastPoolOutput::Demoted(entry) => demoted.push(entry),
            }
        }

        let slow_inputs: Vec<ClassifiedEntry> = classified.slow.into_iter().chain(demoted).collect();
        let slow_outputs = self.run_slow_pool(slow_inputs, &handle, execution).await;
        for (idx, result) in slow_outputs {
            indexed_results[idx] = Some(result);
        }

        let mut prepared = Vec::with_capacity(len);
        for slot in indexed_results {
            match slot {
                Some(Ok(entity)) => prepared.push(entity),
                Some(Err(error)) => errors.push(error),
                None => {}
            }
        }
        (prepared, errors)
    }

    pub(super) async fn prepare_entities(&self, entities: &[Entity], context: &ContextSpec) -> (Vec<PreparedEntity>, Vec<(String, AndromedaError)>) {
        let refs: Vec<&Entity> = entities.iter().collect();
        self.prepare_entity_refs(&refs, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassifiedEntry, classify_batch};
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::json;

    fn test_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine")
    }

    fn entry(idx: usize, value: serde_json::Value) -> ClassifiedEntry {
        ClassifiedEntry {
            idx,
            id: format!("urn:test:{idx}").parse().expect("valid test uri"),
            value,
        }
    }

    fn fast_doc(idx: usize) -> serde_json::Value {
        json!({"id": format!("urn:test:{idx}"), "type": "T", "speed": idx})
    }

    fn slow_doc_with_user_context(idx: usize) -> serde_json::Value {
        // Inline @context that diverges from any core handle.
        json!({
            "@context": {"@vocab": "https://example.com/vocab#"},
            "id": format!("urn:test:{idx}"),
            "type": "T"
        })
    }

    #[tokio::test]
    async fn classify_pure_fast_routes_all_to_fast() {
        let engine = test_engine();
        let handle = engine.batch_context_handle(&ContextSpec::core_only()).await;
        let entries: Vec<ClassifiedEntry> = (0..100).map(|i| entry(i, fast_doc(i))).collect();
        let classified = classify_batch(entries, &handle, &engine);
        pretty_assertions::assert_eq!(classified.fast.len(), 100);
        assert!(classified.slow.is_empty());
    }

    #[tokio::test]
    async fn classify_pure_slow_routes_all_to_slow() {
        let engine = test_engine();
        // A body context that overrides a core term marks the handle
        // ineligible, so all entries route to the slow pool.
        let spec = ContextSpec::none().with_body_context(Some(json!({"observedAt": "https://example.com/own/observedAt"})));
        let handle = engine.batch_context_handle(&spec).await;
        let entries: Vec<ClassifiedEntry> = (0..100).map(|i| entry(i, fast_doc(i))).collect();
        let classified = classify_batch(entries, &handle, &engine);
        assert!(classified.fast.is_empty());
        pretty_assertions::assert_eq!(classified.slow.len(), 100);
    }

    #[tokio::test]
    async fn classify_mixed_preserves_indices() {
        let engine = test_engine();
        let handle = engine.batch_context_handle(&ContextSpec::core_only()).await;
        let entries: Vec<ClassifiedEntry> = (0usize..10)
            .map(|i| {
                if i.is_multiple_of(2) {
                    entry(i, fast_doc(i))
                } else {
                    entry(i, slow_doc_with_user_context(i))
                }
            })
            .collect();
        let classified = classify_batch(entries, &handle, &engine);

        let fast_indices: Vec<usize> = classified.fast.iter().map(|e| e.idx).collect();
        let slow_indices: Vec<usize> = classified.slow.iter().map(|e| e.idx).collect();
        pretty_assertions::assert_eq!(fast_indices, vec![0, 2, 4, 6, 8]);
        pretty_assertions::assert_eq!(slow_indices, vec![1, 3, 5, 7, 9]);
    }
}
