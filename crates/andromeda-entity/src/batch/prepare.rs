//! Turning a batch of request entities into expanded, validated payloads.
//!
//! Entities whose `@context` matches the batch's resolved handle take a
//! synchronous fast path; the rest are expanded through the JSON-LD engine,
//! in bulk where they share the handle and one at a time where they do not.
//! Every entry carries its input index so per-entity errors line up with the
//! caller's order regardless of which pool produced them.

use crate::{
    EntityService,
    batch::execution::{BatchExecution, decide_batch_execution},
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    null_sentinel_admission::NullSentinelAdmission,
    port::blocking_executor::BlockingTask,
    write_admission::admit_expanded_entity,
};
use andromeda_context_commit::entity_record::SearchProjection;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{batch_context::BatchContextHandle, processor::JsonLdEngine},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{attribute_expiry::min_attribute_expires_at, entity_member::extract_expires_at},
};
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{
    mem::{replace, take},
    sync::{Arc, Mutex},
};

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
    /// The members a query filters on, extracted from the expanded payload.
    pub projection: SearchProjection,
    /// Attribute names changed by this request payload.
    pub changed_attributes: Vec<AttributeName>,
    /// Entity expiration timestamp (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
    /// The earliest attribute-instance expiration, when one exists (clause 4.22).
    pub min_attribute_expires_at: Option<DateTime<Utc>>,
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

/// What preparing a batch produced: one slot per input position, filled
/// where preparation succeeded, and the failures against their identifiers.
pub(super) struct PreparedBatch {
    /// The prepared entity at each input position.
    pub(super) slots: Vec<Option<PreparedEntity>>,
    /// The per-entity failures, in position order.
    pub(super) errors: Vec<(Uri, EntityError)>,
}

/// `(input_index, prepared_entity_or_error)` tuple emitted by either pool.
///
/// The error pair is boxed so a successful slot is not sized for the much
/// larger broker error variant.
pub(super) type PreparedSlot = Result<PreparedEntity, Box<(Uri, EntityError)>>;
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

struct FastPreparationTask {
    entries: Vec<ClassifiedEntry>,
    engine: JsonLdEngine,
    handle: BatchContextHandle,
    admission: NullSentinelAdmission,
    outputs: Arc<Mutex<Vec<FastPoolOutput>>>,
}

impl BlockingTask for FastPreparationTask {
    fn run(self: Box<Self>) -> Result<(), EntityError> {
        let prepared = self
            .entries
            .into_iter()
            .map(|entry| process_fast_entry(entry, &self.engine, &self.handle, self.admission))
            .collect::<Vec<_>>();
        self.outputs
            .lock()
            .map_err(|error| EntityError::internal(format!("batch preparation output lock poisoned: {error}")))?
            .extend(prepared);
        Ok(())
    }
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

fn process_fast_entry(entry: ClassifiedEntry, engine: &JsonLdEngine, handle: &BatchContextHandle, admission: NullSentinelAdmission) -> FastPoolOutput {
    match engine.try_fast_expand(&entry.value, handle) {
        Some(expanded) => FastPoolOutput::Done {
            idx: entry.idx,
            result: Box::new(EntityService::build_prepared_entity(entry.id, expanded, admission)),
        },
        None => FastPoolOutput::Demoted(entry),
    }
}

/// Attribute one chunk-level preparation failure to every entity the chunk
/// owned. Clause 5.2.17 requires a real entity URI for each batch error.
fn append_preparation_failures(outputs: &mut Vec<FastPoolOutput>, metadata: Vec<(usize, Uri)>, detail: &str) {
    outputs.extend(metadata.into_iter().map(|(idx, id)| FastPoolOutput::Done {
        idx,
        result: Box::new(Err(Box::new((id, EntityError::internal(detail))))),
    }));
}

impl EntityService {
    pub(super) fn batch_execution(&self, len: usize, fast_count: usize, slow_count: usize) -> BatchExecution {
        decide_batch_execution(
            len,
            fast_count,
            slow_count,
            self.batch_execution.prepare_blocking_threshold,
            self.batch_execution.chunk_concurrency,
            self.batch_execution.concurrency,
        )
    }

    fn build_prepared_entity(id: Uri, expanded: Value, admission: NullSentinelAdmission) -> PreparedSlot {
        // Clauses 5.6.7.4, 5.6.8.4, 5.6.9.4 and 5.6.20.4 each open with
        // "execute the behaviour defined in clause 5.5.4 on JSON-LD
        // validation". The failure attributes itself to this one entity, so
        // clause 5.6.7's multi-status answer reports it without failing its
        // siblings.
        admit_expanded_entity(&expanded, admission).map_err(|error| Box::new((id.clone(), error)))?; // clone: the success path retains the identifier
        let projection = SearchProjection::from_expanded(&expanded).map_err(|error| Box::new((id.clone(), EntityError::from(error))))?; // clone: the success path retains the identifier
        // The names the request changed are the names the payload carries: a
        // batch create or replace writes the whole payload.
        let changed_attributes = projection.attribute_names.clone(); // clone: the projection keeps the names for the row, the change event for its subscribers
        let expires_at = extract_expires_at(&expanded);
        let min_attribute_expires_at = min_attribute_expires_at(&expanded);

        Ok(PreparedEntity {
            id,
            expanded,
            projection,
            changed_attributes,
            expires_at,
            min_attribute_expires_at,
        })
    }

    async fn run_fast_pool(
        &self,
        fast: Vec<ClassifiedEntry>,
        handle: &BatchContextHandle,
        execution: BatchExecution,
        admission: NullSentinelAdmission,
    ) -> Vec<FastPoolOutput> {
        if fast.is_empty() {
            return Vec::new();
        }
        match execution {
            BatchExecution::Inline => fast
                .into_iter()
                .map(|entry| process_fast_entry(entry, &self.engine, handle, admission))
                .collect(),
            BatchExecution::Offload { fast_chunks, .. } => {
                let chunk_size = self.batch_execution.chunk_size.max(1);
                let outputs = Arc::new(Mutex::new(Vec::new()));
                let mut tasks: Vec<Box<dyn BlockingTask>> = Vec::new();
                let mut metadata = Vec::with_capacity(fast.len());
                let mut chunk: Vec<ClassifiedEntry> = Vec::with_capacity(chunk_size);
                let prepare_task = |entries: Vec<ClassifiedEntry>| -> Box<dyn BlockingTask> {
                    Box::new(FastPreparationTask {
                        entries,
                        engine: self.engine.clone(), // clone: task execution shares the engine's internal Arcs.
                        handle: handle.clone(),      // clone: task execution shares the resolved context handle.
                        admission,
                        outputs: Arc::clone(&outputs), // clone: every task writes to the same result collection.
                    })
                };
                for entry in fast {
                    metadata.push((entry.idx, entry.id.clone())); // clone: failure attribution survives moving the entry into a task.
                    chunk.push(entry);
                    if chunk.len() == chunk_size {
                        tasks.push(prepare_task(replace(&mut chunk, Vec::with_capacity(chunk_size))));
                    }
                }
                if !chunk.is_empty() {
                    tasks.push(prepare_task(chunk));
                }
                if let Err(error) = self.blocking_executor.execute(tasks, fast_chunks).await {
                    let mut failures = Vec::new();
                    append_preparation_failures(&mut failures, metadata, &error.to_string());
                    return failures;
                }
                match outputs.lock() {
                    Ok(mut outputs) => take(&mut *outputs),
                    Err(error) => take(&mut *error.into_inner()),
                }
            }
        }
    }

    async fn run_slow_pool(
        &self,
        slow: Vec<ClassifiedEntry>,
        handle: &BatchContextHandle,
        execution: BatchExecution,
        admission: NullSentinelAdmission,
    ) -> Vec<IndexedPrepared> {
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
            if self.engine.entity_context_matches_handle(&value, handle) {
                if let Some(obj) = value.as_object_mut()
                    && !obj.contains_key("@context")
                    && let Some(resolved) = handle.spec().to_context_value()
                {
                    obj.insert("@context".to_string(), resolved);
                }
                hit_pending.push((idx, id, value));
            } else {
                miss_pending.push((idx, id, value));
            }
        }

        let mut results: Vec<IndexedPrepared> = Vec::new();

        if !hit_pending.is_empty() {
            let docs: Vec<Value> = hit_pending.iter().map(|(_, _, v)| v.clone()).collect(); // clone: bulk path needs originals for retry
            let bulk = self.engine.expand_prepared_array(docs, handle.spec().core_policy()).await;
            for ((idx, id, _), expanded) in hit_pending.into_iter().zip(bulk) {
                let result = match expanded {
                    Ok(v) => Self::build_prepared_entity(id, v, admission),
                    Err(error) => Err(Box::new((id, error.into()))),
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
                        Err(error) => return (idx, Err(Box::new((id, error.into())))),
                    };
                    (idx, Self::build_prepared_entity(id, expanded, admission))
                })
                .buffered(concurrency)
                .collect()
                .await;
            results.extend(miss_results);
        }

        results
    }

    /// Expand and screen rendered batch entities, answering one slot per
    /// position below `slot_count`.
    ///
    /// Each entry's `idx` is its position; a position no entry fills -- an
    /// entity that could not be rendered -- stays empty, as does one whose
    /// preparation failed. The failures come back against their entity's own
    /// identifier, in position order.
    pub(super) async fn prepare_payloads(
        &self,
        slot_count: usize,
        entries: Vec<ClassifiedEntry>,
        context: &ContextSpec,
        admission: NullSentinelAdmission,
    ) -> PreparedBatch {
        let handle = self.engine.batch_context_handle(context).await;
        let mut errors: Vec<(Uri, EntityError)> = Vec::new();

        let classified = classify_batch(entries, &handle, &self.engine);
        let execution = self.batch_execution(slot_count, classified.fast.len(), classified.slow.len());

        // Process the fast bucket; collect demotions for the slow pool.
        let fast_outputs = self.run_fast_pool(classified.fast, &handle, execution, admission).await;
        let mut indexed_results: Vec<Option<PreparedSlot>> = (0..slot_count).map(|_| None).collect();
        let mut demoted: Vec<ClassifiedEntry> = Vec::new();
        for output in fast_outputs {
            match output {
                FastPoolOutput::Done { idx, result } => {
                    if idx < slot_count {
                        indexed_results[idx] = Some(*result);
                    } else {
                        // Pool-level error (e.g. JoinError) — record it
                        // without an index slot.
                        if let Err(err) = *result {
                            errors.push(*err);
                        }
                    }
                }
                FastPoolOutput::Demoted(entry) => demoted.push(entry),
            }
        }

        let slow_inputs: Vec<ClassifiedEntry> = classified.slow.into_iter().chain(demoted).collect();
        let slow_outputs = self.run_slow_pool(slow_inputs, &handle, execution, admission).await;
        for (idx, result) in slow_outputs {
            indexed_results[idx] = Some(result);
        }

        let slots = indexed_results
            .into_iter()
            .map(|slot| match slot {
                Some(Ok(entity)) => Some(entity),
                Some(Err(error)) => {
                    errors.push(*error);
                    None
                }
                None => None,
            })
            .collect();
        PreparedBatch { slots, errors }
    }
}

/// Render decoded batch entities to the compact JSON their preparation
/// expands, one entry per entity at its position.
///
/// An entity that cannot be rendered is reported against its identifier and
/// leaves its position without an entry, so the rest of the batch proceeds.
pub(super) fn render_entries<'entities>(entities: impl IntoIterator<Item = &'entities Entity>) -> (Vec<ClassifiedEntry>, Vec<(Uri, EntityError)>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (idx, entity) in entities.into_iter().enumerate() {
        let id = entity.id.clone(); // clone: owned identifier for the classified entry
        match serde_json::to_value(entity) {
            Ok(value) => entries.push(ClassifiedEntry { idx, id, value }),
            Err(error) => errors.push((id, EntityError::Validation(EntityValidationReason::EntityPayload(error)))),
        }
    }
    (entries, errors)
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
