use crate::{
    core::error::AndromedaError,
    jsonld::{BatchContextHandle, ContextSpec, JsonLdEngine},
    query::sql,
    repository::{
        entity::{PreparedEntityRow, decompose_rows, params::PreparedEntityColumns},
        uow::UnitOfWork,
    },
    service::entity::{
        BatchSideEffectJob,
        BatchSideEffectMode,
        EntityService,
        extract_attribute_keys,
        extract_scopes,
        normalize_entity_attributes,
        unwrap_expanded_ref,
        validate_attribute_envelopes,
    },
};
use andromeda_domain::{
    attribute::name::AttributeName,
    entity::Entity,
    entity_type::EntityTypeName,
    expanded::{
        entity_member::{self, extract_expires_at, extract_types},
        geo_property::{extract_location_geometry, extract_observation_space_geometry, extract_operation_space_geometry},
        instance_key::{AttributeInstanceKey, removed_attribute_instances},
        mutation::{
            entity::{append_entity_attrs, merge_entity, update_entity_attrs},
            selection::OverwritePolicy,
        },
    },
    geometry::Geometry,
    notification::{ChangeEvent, SysAttrsSnapshot, TriggerReason},
    uri::Uri,
};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    num::NonZeroUsize,
    sync::Arc,
};
use tokio::{sync::Semaphore, task::JoinSet};

/// Domain-level result of a batch operation.
///
/// Contains the IDs of successfully processed entities and the per-entity
/// errors for those that failed. Converted to HTTP-layer
/// [`BatchOperationResult`](crate::http::error::batch::BatchOperationResult)
/// at the handler boundary.
pub struct BatchResult {
    /// Entity IDs that were successfully processed.
    pub success: Vec<String>,
    /// Per-entity errors: `(entity_id, domain_error)` pairs.
    pub errors: Vec<(String, AndromedaError)>,
}

/// Pre-processed entity data ready for bulk SQL operations.
///
/// Holds all fields extracted during the Rust-side preparation phase
/// (serialization, normalization, JSON-LD expansion, validation). The
/// data is in the exact shape required by the repository bulk methods.
struct PreparedEntity {
    /// Entity identifier string.
    id: String,
    /// Expanded JSON-LD payload (the canonical stored form).
    expanded: Value,
    /// Entity type names extracted from the expanded payload.
    types: Vec<EntityTypeName>,
    /// Scope values extracted from the expanded payload.
    scopes: Vec<String>,
    /// Attribute names changed by this request payload.
    changed_attributes: Vec<AttributeName>,
    /// The `location` geometry, if present.
    location: Option<Geometry>,
    /// The `observationSpace` geometry, if present.
    observation_space: Option<Geometry>,
    /// The `operationSpace` geometry, if present.
    operation_space: Option<Geometry>,
    /// Entity expiration timestamp (ETSI GS CIM 009 clause 4.22).
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Concurrency mode for a batch's preparation phase.
///
/// `Inline` runs everything synchronously in the calling task (small
/// batches). `Offload` parallelises the synchronous fastpath across a
/// pool of `tokio::task::spawn_blocking` workers, while the asynchronous
/// slow path stays on a `stream::buffered`-driven future pool. The two
/// pools run concurrently and emit `(usize, Result)` tuples that the
/// orchestrator merges back into input order.
#[derive(Clone, Copy)]
enum BatchExecution {
    /// Process everything inline in the current task.
    Inline,
    /// Offload preparation to dedicated pools.
    Offload {
        /// Maximum number of concurrent fastpath blocking tasks.
        fast_chunks: NonZeroUsize,
        /// Maximum number of concurrent slow-path async tasks.
        slow_concurrency: NonZeroUsize,
    },
}

/// One entity entered into the batch classifier. Carries the input index
/// so per-entity results can be reassembled into the caller's order
/// regardless of which pool processed the entry.
struct ClassifiedEntry {
    idx: usize,
    id: String,
    value: Value,
}

/// Result of [`classify_batch`]: input entries split into the
/// synchronous fastpath bucket and the asynchronous slow-path bucket.
struct ClassifiedBatch {
    fast: Vec<ClassifiedEntry>,
    slow: Vec<ClassifiedEntry>,
}

/// `(input_index, prepared_entity_or_error)` tuple emitted by either
/// pool. The orchestrator merges these by `input_index` so per-entity
/// errors line up with the caller's input order regardless of which pool
/// produced them.
type PreparedSlot = Result<PreparedEntity, (String, AndromedaError)>;
type IndexedPrepared = (usize, PreparedSlot);
type DeleteChunkOutcome = (Vec<String>, Vec<(String, AndromedaError)>);

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

fn classify_batch(entries: Vec<ClassifiedEntry>, handle: &BatchContextHandle, engine: &JsonLdEngine) -> ClassifiedBatch {
    let mut fast = Vec::new();
    let mut slow = Vec::new();
    let handle_eligible = handle.has_core_term_map();
    for entry in entries {
        if handle_eligible && engine.entity_context_matches_handle(&entry.value, handle) {
            fast.push(entry);
        } else {
            slow.push(entry);
        }
    }
    ClassifiedBatch { fast, slow }
}

fn decide_batch_execution(
    len: usize,
    _fast_count: usize,
    _slow_count: usize,
    blocking_threshold: usize,
    fast_chunk_concurrency: usize,
    slow_concurrency: usize,
) -> BatchExecution {
    if len < blocking_threshold {
        return BatchExecution::Inline;
    }
    let fast_chunks = NonZeroUsize::new(fast_chunk_concurrency.max(1)).unwrap_or(NonZeroUsize::MIN);
    let slow_concurrency = NonZeroUsize::new(slow_concurrency.max(1)).unwrap_or(NonZeroUsize::MIN);
    BatchExecution::Offload { fast_chunks, slow_concurrency }
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

struct BatchEffectEntity {
    changed_attributes: Vec<AttributeName>,
    expanded: Value,
    types: Vec<EntityTypeName>,
}

struct IndexedBatchEffectEntity {
    changed_attributes: Vec<AttributeName>,
    payload_index: usize,
    types: Vec<EntityTypeName>,
}

/// Build a [`PreparedEntityRow`] from an already-merged expanded payload.
///
/// Used by update/merge/upsert flows where the caller has already produced the
/// final merged payload and needs a row for `save_batch`.
fn row_from_expanded(id: String, expanded: Value) -> (PreparedEntityRow, Vec<EntityTypeName>) {
    let types = extract_types(&expanded);
    let scope_paths_json = Value::Array(
        entity_member::extract_scopes(&expanded)
            .into_iter()
            .map(|s| Value::String(sql::scope_to_ltree(&s)))
            .collect(),
    );
    let location = extract_location_geometry(&expanded);
    let observation_space = extract_observation_space_geometry(&expanded);
    let operation_space = extract_operation_space_geometry(&expanded);
    let expires_at = extract_expires_at(&expanded);

    (
        PreparedEntityRow {
            id,
            types_json: Value::Array(types.iter().map(|entity_type| Value::String(entity_type.to_string())).collect()),
            scope_paths_json,
            payload: expanded,
            location,
            observation_space,
            operation_space,
            expires_at,
        },
        types,
    )
}

impl EntityService {
    const fn is_large_batch(&self, len: usize) -> bool {
        len >= self.batch_large_threshold
    }

    fn batch_chunk_len(&self, len: usize) -> usize {
        if self.is_large_batch(len) {
            self.batch_chunk_size.min(len).max(1)
        } else {
            len.max(1)
        }
    }

    fn batch_execution(&self, len: usize, fast_count: usize, slow_count: usize) -> BatchExecution {
        decide_batch_execution(
            len,
            fast_count,
            slow_count,
            self.batch_prepare_blocking_threshold,
            self.batch_chunk_concurrency,
            self.batch_concurrency,
        )
    }

    fn build_prepared_entity(id: String, expanded: Value) -> PreparedSlot {
        validate_attribute_envelopes(&expanded).map_err(|err| (id.clone(), err))?;
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
        // diverges from the handle (FastPath::Miss) keep their own
        // context and require per-entity expansion against
        // [`ContextSpec::apply_context`]; the rest can ride a single
        // bulk JSON-LD expand call.
        let mut hit_pending: Vec<(usize, String, Value)> = Vec::new();
        let mut miss_pending: Vec<(usize, String, Value)> = Vec::new();
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

        // Bulk-expand the FastPath::Hit bucket through one json-ld call.
        if !hit_pending.is_empty() {
            let docs: Vec<Value> = hit_pending.iter().map(|(_, _, v)| v.clone()).collect(); // clone: bulk path needs originals for retry
            let bulk = self.engine.expand_prepared_array(docs, handle.spec().core_policy()).await;
            for ((idx, id, _), expanded) in hit_pending.into_iter().zip(bulk) {
                let result = match expanded {
                    Ok(v) => Self::build_prepared_entity(id, v),
                    Err(err) => Err((id, err.into())),
                };
                results.push((idx, result));
            }
        }

        // FastPath::Miss entries still need per-entity expand because each
        // carries its own divergent `@context`.
        if !miss_pending.is_empty() {
            let concurrency = match execution {
                BatchExecution::Inline => 1,
                BatchExecution::Offload { slow_concurrency, .. } => slow_concurrency.get(),
            };
            let miss_results: Vec<IndexedPrepared> = stream::iter(miss_pending)
                .map(|(idx, id, value)| async move {
                    let expanded = match self.engine.expand(value, handle.spec()).await {
                        Ok(v) => v,
                        Err(err) => return (idx, Err((id, err.into()))),
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

    async fn prepare_entity_refs(&self, entities: &[&Entity], context: &ContextSpec) -> (Vec<PreparedEntity>, Vec<(String, AndromedaError)>) {
        let len = entities.len();
        let handle = self.engine.batch_context_handle(context).await;

        // Serialize all entities to JSON upfront so per-entity serialization
        // failures are reported and the rest of the batch can proceed.
        let mut entries: Vec<ClassifiedEntry> = Vec::with_capacity(len);
        let mut errors: Vec<(String, AndromedaError)> = Vec::new();
        for (idx, entity) in entities.iter().enumerate() {
            let id = entity.id.to_string();
            match serde_json::to_value(*entity) {
                Ok(value) => entries.push(ClassifiedEntry { idx, id, value }),
                Err(err) => errors.push((id, AndromedaError::entity_validation(format!("Invalid entity payload: {}", err)))),
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

    async fn prepare_entities(&self, entities: &[Entity], context: &ContextSpec) -> (Vec<PreparedEntity>, Vec<(String, AndromedaError)>) {
        let refs: Vec<&Entity> = entities.iter().collect();
        self.prepare_entity_refs(&refs, context).await
    }

    fn drain_effect_entities(
        entity_data: &mut HashMap<String, BatchEffectEntity>,
        ids: &[String],
        trigger_reason: TriggerReason,
    ) -> (BatchSideEffectJob, Vec<String>) {
        let mut change_events = Vec::with_capacity(ids.len());
        let mut success = Vec::with_capacity(ids.len());
        let mut temporal_entities = Vec::with_capacity(ids.len());

        for id in ids {
            let Some(effect) = entity_data.remove(id) else {
                continue;
            };

            let id_arc: Arc<str> = Arc::from(id.as_str());
            let expanded_arc = Arc::new(effect.expanded);
            temporal_entities.push((Arc::clone(&id_arc), Arc::clone(&expanded_arc)));
            match id.parse::<Uri>() {
                Ok(entity_uri) => {
                    change_events.push(
                        ChangeEvent::new(entity_uri, effect.types, trigger_reason)
                            .with_changed_attributes(effect.changed_attributes)
                            .with_current_payload(expanded_arc),
                    );
                }
                Err(_error) => {
                    tracing::error!(entity_id = %id, event = "entity_uri_parse_failed");
                }
            }
            success.push(id_arc.to_string());
        }

        (
            BatchSideEffectJob {
                temporal_entities,
                change_events,
            },
            success,
        )
    }

    fn drain_indexed_effect_entities(
        entity_data: &mut HashMap<String, IndexedBatchEffectEntity>,
        payloads: &mut [Option<Value>],
        ids: &[String],
        trigger_reason: TriggerReason,
    ) -> (BatchSideEffectJob, Vec<String>) {
        let mut change_events = Vec::with_capacity(ids.len());
        let mut success = Vec::with_capacity(ids.len());
        let mut temporal_entities = Vec::with_capacity(ids.len());

        for id in ids {
            let Some(effect) = entity_data.remove(id) else {
                continue;
            };

            let Some(expanded) = payloads.get_mut(effect.payload_index).and_then(Option::take) else {
                tracing::error!(entity_id = %id, event = "batch_effect_payload_missing");
                continue;
            };

            let id_arc: Arc<str> = Arc::from(id.as_str());
            let expanded_arc = Arc::new(expanded);
            temporal_entities.push((Arc::clone(&id_arc), Arc::clone(&expanded_arc)));
            match id.parse::<Uri>() {
                Ok(entity_uri) => {
                    change_events.push(
                        ChangeEvent::new(entity_uri, effect.types, trigger_reason)
                            .with_changed_attributes(effect.changed_attributes)
                            .with_current_payload(expanded_arc),
                    );
                }
                Err(_error) => {
                    tracing::error!(entity_id = %id, event = "entity_uri_parse_failed");
                }
            }
            success.push(id_arc.to_string());
        }

        (
            BatchSideEffectJob {
                temporal_entities,
                change_events,
            },
            success,
        )
    }

    fn drain_upsert_indexed_effect_entities(
        entity_data: &mut HashMap<String, IndexedBatchEffectEntity>,
        payloads: &mut [Option<Value>],
        upserted: Vec<(String, bool)>,
    ) -> (BatchSideEffectJob, Vec<String>, Vec<String>) {
        let mut change_events = Vec::with_capacity(upserted.len());
        let mut created = Vec::new();
        let mut success = Vec::with_capacity(upserted.len());
        let mut temporal_entities = Vec::with_capacity(upserted.len());

        for (id, was_created) in upserted {
            let Some(effect) = entity_data.remove(&id) else {
                continue;
            };

            let Some(expanded) = payloads.get_mut(effect.payload_index).and_then(Option::take) else {
                tracing::error!(entity_id = %id, event = "batch_effect_payload_missing");
                continue;
            };

            let id_arc: Arc<str> = Arc::from(id.as_str());
            let trigger_reason = if was_created {
                created.push(id.clone());
                TriggerReason::EntityCreated
            } else {
                TriggerReason::EntityUpdated
            };
            let expanded_arc = Arc::new(expanded);
            temporal_entities.push((Arc::clone(&id_arc), Arc::clone(&expanded_arc)));
            match id.parse::<Uri>() {
                Ok(entity_uri) => {
                    change_events.push(
                        ChangeEvent::new(entity_uri, effect.types, trigger_reason)
                            .with_changed_attributes(effect.changed_attributes)
                            .with_current_payload(expanded_arc),
                    );
                }
                Err(_error) => {
                    tracing::error!(entity_id = %id, event = "entity_uri_parse_failed");
                }
            }
            success.push(id);
        }

        (
            BatchSideEffectJob {
                temporal_entities,
                change_events,
            },
            success,
            created,
        )
    }

    fn dedupe_create_entities(entities: &[Entity]) -> (Vec<&Entity>, Vec<(String, AndromedaError)>) {
        let mut deduped = Vec::with_capacity(entities.len());
        let mut errors = Vec::new();
        let mut seen = HashSet::with_capacity(entities.len());

        for entity in entities {
            let id = entity.id.to_string();
            if seen.insert(id.clone()) {
                deduped.push(entity);
            } else {
                errors.push((id.clone(), AndromedaError::entity_already_exists(&id)));
            }
        }

        (deduped, errors)
    }

    fn dedupe_upsert_entities(entities: &[Entity]) -> Vec<&Entity> {
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

    async fn batch_create_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        original_batch_size: usize,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, AndromedaError> {
        let _ = original_batch_size;
        let (prepared, mut errors) = self.prepare_entity_refs(entities, context).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let chunk_ids: Vec<String> = prepared.iter().map(|p| p.id.clone()).collect();
        let _id_guards = self.acquire_id_locks(&chunk_ids).await;

        let mut entity_data = HashMap::with_capacity(prepared.len());
        let mut columns = PreparedEntityColumns::with_capacity(prepared.len());
        for prepared_entity in prepared {
            let key = prepared_entity.id.clone();
            match entity_data.entry(key) {
                Entry::Occupied(entry) => {
                    let id = entry.key().clone();
                    errors.push((id.clone(), AndromedaError::entity_already_exists(&id)));
                }
                Entry::Vacant(entry) => {
                    let payload_index = columns.payloads.len();
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
                    } = prepared_entity;
                    columns.ids.push(id);
                    columns
                        .types_json
                        .push(Value::Array(types.iter().map(|entity_type| Value::String(entity_type.to_string())).collect()));
                    columns
                        .scope_paths_json
                        .push(Value::Array(scopes.into_iter().map(|s| Value::String(sql::scope_to_ltree(&s))).collect()));
                    columns.push_payload(expanded)?;
                    columns.push_geometries(location.as_ref(), observation_space.as_ref(), operation_space.as_ref())?;
                    columns.expires_ats.push(expires_at);
                    entry.insert(IndexedBatchEffectEntity {
                        changed_attributes,
                        payload_index,
                        types,
                    });
                }
            }
        }

        // Single-transaction path: entity write and temporal-side-effect
        // write (inline) / queue enqueue (async) share one commit. Halves
        // the fsync cost of the inline create path and guarantees
        // at-least-once delivery of the async path without an orphan
        // queue row.
        let mut uow = self.uow.begin().await?;
        let inserted_ids = uow.create_entities_batch(&columns).await?;
        self.report_missing_inserts(&inserted_ids, &entity_data, &mut errors);
        let mut payloads = columns.payloads.into_iter().map(Some).collect::<Vec<_>>();
        let (job, success) = Self::drain_indexed_effect_entities(&mut entity_data, &mut payloads, &inserted_ids, TriggerReason::EntityCreated);

        self.commit_chunk_side_effects(uow, job, mode).await?;

        Ok(BatchResult { success, errors })
    }

    async fn commit_chunk_side_effects(&self, mut uow: UnitOfWork, job: BatchSideEffectJob, mode: BatchSideEffectMode) -> Result<(), AndromedaError> {
        match mode {
            BatchSideEffectMode::Inline => {
                uow.record_entities_batch(&job.temporal_entities).await?;
                uow.commit().await?;
                for event in job.change_events {
                    self.emit_change(event);
                }
            }
            BatchSideEffectMode::Enqueue => {
                self.enqueue_batch_side_effects(&mut uow, &job).await?;
                uow.commit().await?;
            }
        }
        Ok(())
    }

    fn report_missing_inserts(
        &self,
        inserted_ids: &[String],
        entity_data: &HashMap<String, IndexedBatchEffectEntity>,
        errors: &mut Vec<(String, AndromedaError)>,
    ) {
        let inserted_set: HashSet<&str> = inserted_ids.iter().map(String::as_str).collect();
        for id in entity_data.keys() {
            if !inserted_set.contains(id.as_str()) {
                errors.push((id.clone(), AndromedaError::entity_already_exists(id)));
            }
        }
    }

    async fn batch_update_chunk(
        &self,
        entities: &[Entity],
        context: &ContextSpec,
        overwrite: OverwritePolicy,
        original_batch_size: usize,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, AndromedaError> {
        let _ = original_batch_size;
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<String> = prepared.iter().map(|entity| entity.id.clone()).collect();
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent updates for the same id can't lose each other's writes.
        let _id_guards = self.acquire_id_locks(&prepared_ids).await;
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut effect_entities = HashMap::with_capacity(prepared.len());
        let mut save_rows = Vec::with_capacity(prepared.len());

        for prepared_entity in prepared {
            let id = prepared_entity.id;
            let Some(mut existing) = existing_map.get(&id).cloned() else {
                errors.push((id.clone(), AndromedaError::entity_not_found(&id)));
                continue;
            };

            let attrs = unwrap_expanded_ref(&prepared_entity.expanded);
            if overwrite == OverwritePolicy::Preserve {
                let skipped = has_overlapping_attrs(&existing, attrs);
                if let Err(error) = append_entity_attrs(&mut existing, attrs, OverwritePolicy::Preserve) {
                    errors.push((id, error.into()));
                    continue;
                }
                if skipped {
                    errors.push((
                        id.clone(),
                        AndromedaError::entity_validation("Some attributes already exist and were not updated due to noOverwrite"),
                    ));
                }
            } else {
                if let Err(error) = update_entity_attrs(&mut existing, attrs) {
                    errors.push((id, error.into()));
                    continue;
                }
                if let Err(error) = append_entity_attrs(&mut existing, attrs, OverwritePolicy::Overwrite) {
                    errors.push((id, error.into()));
                    continue;
                }
            }

            let (row, row_types) = row_from_expanded(id.clone(), existing);
            let effect = BatchEffectEntity {
                changed_attributes: prepared_entity.changed_attributes,
                expanded: prepared_entity.expanded,
                types: row_types,
            };
            effect_entities.insert(id, effect);
            save_rows.push(row);
        }

        let save_columns = decompose_rows(save_rows)?;

        let mut uow = self.uow.begin().await?;
        let saved_ids = uow.save_entities_batch(&save_columns).await?;
        let (job, success) = Self::drain_effect_entities(&mut effect_entities, &saved_ids, TriggerReason::EntityUpdated);
        self.commit_chunk_side_effects(uow, job, mode).await?;
        Ok(BatchResult { success, errors })
    }

    async fn batch_merge_chunk(
        &self,
        entities: &[Entity],
        context: &ContextSpec,
        original_batch_size: usize,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, AndromedaError> {
        let _ = original_batch_size;
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<String> = prepared.iter().map(|entity| entity.id.clone()).collect();
        // Hold the per-id locks across the read-modify-write window so two
        // concurrent merges for the same id can't lose each other's writes.
        let _id_guards = self.acquire_id_locks(&prepared_ids).await;
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut effect_entities = HashMap::with_capacity(prepared.len());
        let mut save_rows = Vec::with_capacity(prepared.len());

        for prepared_entity in prepared {
            let id = prepared_entity.id;
            let Some(mut existing) = existing_map.get(&id).cloned() else {
                errors.push((id.clone(), AndromedaError::entity_not_found(&id)));
                continue;
            };

            if let Err(error) = merge_entity(&mut existing, &prepared_entity.expanded) {
                errors.push((id, error.into()));
                continue;
            }

            let (row, row_types) = row_from_expanded(id.clone(), existing);
            let effect = BatchEffectEntity {
                changed_attributes: prepared_entity.changed_attributes,
                expanded: prepared_entity.expanded,
                types: row_types,
            };
            effect_entities.insert(id, effect);
            save_rows.push(row);
        }

        let save_columns = decompose_rows(save_rows)?;

        let mut uow = self.uow.begin().await?;
        let saved_ids = uow.save_entities_batch(&save_columns).await?;
        let (job, success) = Self::drain_effect_entities(&mut effect_entities, &saved_ids, TriggerReason::EntityUpdated);
        self.commit_chunk_side_effects(uow, job, mode).await?;
        Ok(BatchResult { success, errors })
    }

    async fn batch_upsert_replace_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(String, AndromedaError)>,
        original_batch_size: usize,
        mode: BatchSideEffectMode,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        let _ = original_batch_size;
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

        let chunk_ids: Vec<String> = prepared.iter().map(|p| p.id.clone()).collect();
        let _id_guards = self.acquire_id_locks(&chunk_ids).await;

        let mut entity_data = HashMap::with_capacity(prepared.len());
        let mut columns = PreparedEntityColumns::with_capacity(prepared.len());
        for prepared_entity in prepared {
            let payload_index = columns.payloads.len();
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
            } = prepared_entity;
            let types_json = Value::Array(types.iter().map(|entity_type| Value::String(entity_type.to_string())).collect());
            entity_data.insert(
                id.clone(),
                IndexedBatchEffectEntity {
                    changed_attributes,
                    payload_index,
                    types,
                },
            );
            columns.ids.push(id);
            columns.types_json.push(types_json);
            columns
                .scope_paths_json
                .push(Value::Array(scopes.into_iter().map(|s| Value::String(sql::scope_to_ltree(&s))).collect()));
            columns.push_payload(expanded)?;
            columns.push_geometries(location.as_ref(), observation_space.as_ref(), operation_space.as_ref())?;
            columns.expires_ats.push(expires_at);
        }

        // Replace-mode upsert overwrites `entities.payload` wholesale, so any
        // attribute the replacement omits is deleted (NGSI-LD 5.6.8) and needs
        // a tombstone. The prior payloads are read inside the same transaction
        // that writes the replacements, so the diff cannot be computed against
        // a snapshot another writer has already moved past.
        //
        // The diff is taken against `entities.payload` rather than the TEA
        // catalogue on purpose: TEA rows outlive the attributes they describe
        // (deletion writes a tombstone, it does not drop the row), so diffing
        // against them would re-tombstone already-deleted attributes on every
        // upsert.
        let mut uow = self.uow.begin().await?;
        let previous = uow.get_entities(&chunk_ids).await?;
        let removals = Self::collect_replaced_attribute_removals(&entity_data, &columns.payloads, &previous);

        let upserted = uow.upsert_entities_batch(&columns).await?;
        let mut payloads = columns.payloads.into_iter().map(Some).collect::<Vec<_>>();
        let (job, success, created) = Self::drain_upsert_indexed_effect_entities(&mut entity_data, &mut payloads, upserted);
        for (id, removed) in &removals {
            uow.record_removed_attributes(id, removed).await?;
        }
        self.commit_chunk_side_effects(uow, job, mode).await?;
        Ok((BatchResult { success, errors: Vec::new() }, created))
    }

    /// Attribute instances each replaced entity drops, keyed by entity id.
    ///
    /// Entities absent from `previous` were created rather than replaced and
    /// contribute nothing.
    fn collect_replaced_attribute_removals(
        entity_data: &HashMap<String, IndexedBatchEffectEntity>,
        payloads: &[Value],
        previous: &HashMap<String, Value>,
    ) -> Vec<(String, Vec<AttributeInstanceKey>)> {
        entity_data
            .iter()
            .filter_map(|(id, entry)| {
                let prior = previous.get(id)?;
                let replacement = payloads.get(entry.payload_index)?;
                let removed = removed_attribute_instances(prior, replacement);
                (!removed.is_empty()).then(|| (id.clone(), removed)) // clone: owned key for the returned pair
            })
            .collect()
    }

    async fn batch_upsert_update_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(String, AndromedaError)>,
        original_batch_size: usize,
        mode: BatchSideEffectMode,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        let _ = original_batch_size;
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

        let prepared_ids: Vec<String> = prepared.iter().map(|entity| entity.id.clone()).collect();
        // Hold the per-id locks across the read-modify-write window: concurrent
        // upsert-update calls for the same id must not race the snapshot
        // read, the merge, and the write.
        let _id_guards = self.acquire_id_locks(&prepared_ids).await;
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut created_effects = HashMap::new();
        let mut created_columns = PreparedEntityColumns::with_capacity(prepared.len());
        let mut updated_effects = HashMap::new();
        let mut updated_rows = Vec::new();

        for prepared_entity in prepared {
            let id = prepared_entity.id.clone();
            if let Some(existing) = existing_map.get(&id) {
                let mut merged = existing.clone();
                if let Err(error) = merge_entity(&mut merged, &prepared_entity.expanded) {
                    errors.push((id, error.into()));
                    continue;
                }

                let (row, row_types) = row_from_expanded(id.clone(), merged);
                let effect = BatchEffectEntity {
                    changed_attributes: prepared_entity.changed_attributes,
                    expanded: prepared_entity.expanded,
                    types: row_types,
                };
                updated_effects.insert(id, effect);
                updated_rows.push(row);
            } else {
                let payload_index = created_columns.payloads.len();
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
                } = prepared_entity;
                let types_json = Value::Array(types.iter().map(|entity_type| Value::String(entity_type.to_string())).collect());
                created_effects.insert(
                    id.clone(),
                    IndexedBatchEffectEntity {
                        changed_attributes,
                        payload_index,
                        types,
                    },
                );
                created_columns.ids.push(id);
                created_columns.types_json.push(types_json);
                created_columns
                    .scope_paths_json
                    .push(Value::Array(scopes.into_iter().map(|s| Value::String(sql::scope_to_ltree(&s))).collect()));
                created_columns.push_payload(expanded)?;
                created_columns.push_geometries(location.as_ref(), observation_space.as_ref(), operation_space.as_ref())?;
                created_columns.expires_ats.push(expires_at);
            }
        }

        let updated_columns = if updated_rows.is_empty() { None } else { Some(decompose_rows(updated_rows)?) };

        let mut uow = self.uow.begin().await?;
        let created_ids = if created_columns.ids.is_empty() {
            Vec::new()
        } else {
            uow.create_entities_batch(&created_columns).await?
        };
        let updated_ids = if let Some(cols) = &updated_columns {
            uow.save_entities_batch(cols).await?
        } else {
            Vec::new()
        };

        let mut created_payloads = created_columns.payloads.into_iter().map(Some).collect::<Vec<_>>();
        let (created_job, mut success, created) = Self::drain_upsert_indexed_effect_entities(
            &mut created_effects,
            &mut created_payloads,
            created_ids.into_iter().map(|id| (id, true)).collect(),
        );
        let (updated_job, updated_success) = Self::drain_effect_entities(&mut updated_effects, &updated_ids, TriggerReason::EntityUpdated);
        success.extend(updated_success);

        let mut temporal_entities = created_job.temporal_entities;
        temporal_entities.extend(updated_job.temporal_entities);
        let mut change_events = created_job.change_events;
        change_events.extend(updated_job.change_events);

        let job = BatchSideEffectJob {
            temporal_entities,
            change_events,
        };
        self.commit_chunk_side_effects(uow, job, mode).await?;

        Ok((BatchResult { success, errors: Vec::new() }, created))
    }

    // -------------------------------------------------------------------------
    // Batch create (NGSI-LD 5.6.7)
    // -------------------------------------------------------------------------

    /// Create multiple entities in a single batch operation (NGSI-LD 5.6.7).
    pub async fn batch_create(&self, entities: &[Entity], context: &ContextSpec, mode: BatchSideEffectMode) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let (deduped, mut errors) = Self::dedupe_create_entities(entities);
        if deduped.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let chunk_len = self.batch_chunk_len(deduped.len());
        let batch_size = entities.len();
        let chunk_results: Vec<Result<BatchResult, AndromedaError>> = stream::iter(deduped.chunks(chunk_len))
            .map(|chunk| async move { self.batch_create_chunk(chunk, context, batch_size, mode).await })
            .buffered(self.batch_chunk_write_concurrency)
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

    // -------------------------------------------------------------------------
    // Batch delete (NGSI-LD 5.6.10)
    // -------------------------------------------------------------------------

    /// Delete multiple entities by ID in a single batch operation (NGSI-LD 5.6.10).
    pub async fn batch_delete(&self, entity_ids: &[String], mode: BatchSideEffectMode) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let mut seen = HashSet::new();
        let mut unique_ids = Vec::new();
        let mut errors = Vec::new();
        for id in entity_ids {
            if !seen.insert(id.as_str()) {
                errors.push((id.clone(), AndromedaError::entity_not_found(id)));
            } else {
                unique_ids.push(id.clone());
            }
        }

        let chunk_len = self.batch_chunk_len(unique_ids.len());
        let batch_size = entity_ids.len();
        let chunk_results: Vec<Result<DeleteChunkOutcome, AndromedaError>> = stream::iter(unique_ids.chunks(chunk_len))
            .map(|chunk| async move { self.batch_delete_chunk(chunk, batch_size, mode).await })
            .buffered(self.batch_chunk_write_concurrency)
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

    async fn batch_delete_chunk(&self, chunk: &[String], original_batch_size: usize, mode: BatchSideEffectMode) -> Result<DeleteChunkOutcome, AndromedaError> {
        let _ = original_batch_size;
        let chunk_ids: Vec<String> = chunk.to_vec();
        let _id_guards = self.acquire_id_locks(&chunk_ids).await;
        let (deleted, uow_slot) = match mode {
            BatchSideEffectMode::Inline => (self.repo.delete_batch(chunk).await?, None),
            BatchSideEffectMode::Enqueue => {
                let mut uow = self.uow.begin().await?;
                let deleted = uow.delete_entities_batch(chunk).await?;
                (deleted, Some(uow))
            }
        };
        let deleted_set: HashSet<&str> = deleted.iter().map(|entity| entity.id.as_str()).collect();
        let mut errors = Vec::new();
        for id in chunk {
            if !deleted_set.contains(id.as_str()) {
                errors.push((id.clone(), AndromedaError::entity_not_found(id)));
            }
        }

        let mut success = Vec::new();
        let mut change_events = Vec::with_capacity(deleted.len() * 2);
        for deleted_entity in deleted {
            let entity_types = extract_types(&deleted_entity.payload);
            let attr_keys = extract_attribute_keys(&deleted_entity.payload);
            let sys_attrs = SysAttrsSnapshot {
                created_at: deleted_entity.created_at,
                modified_at: deleted_entity.modified_at,
                expires_at: deleted_entity.expires_at,
            };

            let entity_uri: Uri = match deleted_entity.id.parse() {
                Ok(uri) => uri,
                Err(_error) => {
                    tracing::error!(entity_id = %deleted_entity.id, event = "entity_uri_parse_failed");
                    success.push(deleted_entity.id);
                    continue;
                }
            };

            if !attr_keys.is_empty() {
                change_events.push(
                    ChangeEvent::new(entity_uri.clone(), entity_types.clone(), TriggerReason::AttributeDeleted)
                        .with_changed_attributes(attr_keys)
                        .with_snapshot(deleted_entity.payload.clone())
                        .with_previous(deleted_entity.payload.clone())
                        .with_sys_attrs(Some(sys_attrs.clone())),
                );
            }

            change_events.push(
                ChangeEvent::new(entity_uri, entity_types, TriggerReason::EntityDeleted)
                    .with_snapshot(deleted_entity.payload.clone())
                    .with_previous(deleted_entity.payload)
                    .with_sys_attrs(Some(sys_attrs)),
            );
            success.push(deleted_entity.id);
        }

        let job = BatchSideEffectJob {
            temporal_entities: Vec::new(),
            change_events,
        };
        if let Some(mut uow) = uow_slot {
            self.enqueue_batch_side_effects(&mut uow, &job).await?;
            uow.commit().await?;
        } else {
            self.apply_inline_side_effects(job);
        }

        Ok((success, errors))
    }

    // -------------------------------------------------------------------------
    // Batch update (NGSI-LD 5.6.9)
    // -------------------------------------------------------------------------

    /// Update attributes of multiple entities in a single batch operation (NGSI-LD 5.6.9).
    pub async fn batch_update(
        &self,
        entities: &[Entity],
        context: &ContextSpec,
        overwrite: OverwritePolicy,
        mode: BatchSideEffectMode,
    ) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let chunk_len = self.batch_chunk_len(entities.len());
        let batch_size = entities.len();
        let chunk_results: Vec<Result<BatchResult, AndromedaError>> = stream::iter(entities.chunks(chunk_len))
            .map(|chunk| async move { self.batch_update_chunk(chunk, context, overwrite, batch_size, mode).await })
            .buffered(self.batch_chunk_write_concurrency)
            .collect()
            .await;

        let mut success = Vec::new();
        let mut errors = Vec::new();
        for result in chunk_results {
            let chunk_result = result?;
            success.extend(chunk_result.success);
            errors.extend(chunk_result.errors);
        }

        Ok(BatchResult { success, errors })
    }

    // -------------------------------------------------------------------------
    // Batch merge
    // -------------------------------------------------------------------------

    /// Merge partial updates into multiple entities in a single batch operation.
    pub async fn batch_merge(&self, entities: &[Entity], context: &ContextSpec, mode: BatchSideEffectMode) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let chunk_len = self.batch_chunk_len(entities.len());
        let batch_size = entities.len();
        let chunk_results: Vec<Result<BatchResult, AndromedaError>> = stream::iter(entities.chunks(chunk_len))
            .map(|chunk| async move { self.batch_merge_chunk(chunk, context, batch_size, mode).await })
            .buffered(self.batch_chunk_write_concurrency)
            .collect()
            .await;

        let mut success = Vec::new();
        let mut errors = Vec::new();
        for result in chunk_results {
            let chunk_result = result?;
            success.extend(chunk_result.success);
            errors.extend(chunk_result.errors);
        }

        Ok(BatchResult { success, errors })
    }

    // -------------------------------------------------------------------------
    // Batch upsert (NGSI-LD 5.6.8)
    // -------------------------------------------------------------------------

    /// Create or update multiple entities in a single batch operation (NGSI-LD 5.6.8).
    pub async fn batch_upsert(
        &self,
        entities: &[Entity],
        context: &ContextSpec,
        update_mode: bool,
        mode: BatchSideEffectMode,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        self.batch_activity.record_activity();
        let deduped = Self::dedupe_upsert_entities(entities);
        let chunk_len = self.batch_chunk_len(deduped.len());
        let batch_size = entities.len();

        type ChunkOutcome = (BatchResult, Vec<(String, AndromedaError)>, Vec<String>);
        let chunk_results: Vec<Result<ChunkOutcome, AndromedaError>> = stream::iter(deduped.chunks(chunk_len))
            .map(|chunk| async move {
                let mut chunk_errors = Vec::new();
                let outcome = if update_mode {
                    self.batch_upsert_update_chunk(chunk, context, &mut chunk_errors, batch_size, mode).await
                } else {
                    self.batch_upsert_replace_chunk(chunk, context, &mut chunk_errors, batch_size, mode).await
                };
                outcome.map(|(result, created)| (result, chunk_errors, created))
            })
            .buffered(self.batch_chunk_write_concurrency)
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

/// Check if any non-@-prefixed attribute keys in `attrs` already exist in `entity`.
fn has_overlapping_attrs(entity: &Value, attrs: &Value) -> bool {
    let entity_obj = match entity {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        _other => None,
    };
    let attrs_obj = attrs.as_object();

    if let (Some(entity_obj), Some(attrs_obj)) = (entity_obj, attrs_obj) {
        attrs_obj.keys().any(|key| !key.starts_with('@') && entity_obj.contains_key(key))
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::jsonld::JsonLdEngine;
    use serde_json::json;

    fn test_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine")
    }

    fn entry(idx: usize, value: serde_json::Value) -> ClassifiedEntry {
        ClassifiedEntry {
            idx,
            id: format!("urn:test:{idx}"),
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
        // Step 5: a body context that overrides a core term marks the
        // handle Ineligible, so all entries route to the slow pool.
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

    #[test]
    fn decide_batch_execution_below_threshold_returns_inline() {
        let exec = decide_batch_execution(64, 64, 0, 128, 4, 8);
        assert!(matches!(exec, BatchExecution::Inline));
    }

    #[test]
    fn decide_batch_execution_above_threshold_returns_offload() {
        let exec = decide_batch_execution(1024, 1000, 24, 128, 4, 8);
        match exec {
            BatchExecution::Offload { fast_chunks, slow_concurrency } => {
                pretty_assertions::assert_eq!(fast_chunks.get(), 4);
                pretty_assertions::assert_eq!(slow_concurrency.get(), 8);
            }
            BatchExecution::Inline => panic!("expected Offload"),
        }
    }

    #[test]
    fn decide_batch_execution_clamps_zero_concurrency_to_one() {
        let exec = decide_batch_execution(1024, 1024, 0, 128, 0, 0);
        match exec {
            BatchExecution::Offload { fast_chunks, slow_concurrency } => {
                pretty_assertions::assert_eq!(fast_chunks.get(), 1);
                pretty_assertions::assert_eq!(slow_concurrency.get(), 1);
            }
            BatchExecution::Inline => panic!("expected Offload"),
        }
    }
}
