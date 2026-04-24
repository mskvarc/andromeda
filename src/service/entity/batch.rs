use crate::{
    core::error::AndromedaError,
    domain::{
        entity::Entity,
        expanded::{self, append_entity_attrs, merge_entity, update_entity_attrs},
        geometry::Wkb,
        notification::{ChangeEvent, SysAttrsSnapshot, TriggerReason},
        uri::Uri,
    },
    jsonld::ContextSpec,
    query::sql,
    repository::entity::{PreparedEntityRow, decompose_rows, params::PreparedEntityColumns},
    service::entity::{
        BatchSideEffectJob,
        EntityService,
        extract_attribute_keys,
        extract_entity_types,
        extract_scopes,
        normalize_entity_attributes,
        unwrap_expanded_ref,
        validate_attribute_envelopes,
    },
};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use tokio::task;

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
    /// Entity type IRIs extracted from the expanded payload.
    types: Vec<String>,
    /// Scope values extracted from the expanded payload.
    scopes: Vec<String>,
    /// Attribute keys changed by this request payload.
    changed_attributes: Vec<String>,
    /// Canonical WKB for `location`, if present.
    location: Option<Wkb>,
    /// Canonical WKB for `observationSpace`, if present.
    observation_space: Option<Wkb>,
    /// Canonical WKB for `operationSpace`, if present.
    operation_space: Option<Wkb>,
    /// Entity expiration timestamp (ETSI GS CIM 009 clause 4.22).
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Copy)]
enum PrepareExecution {
    Inline,
    Offload,
}

struct BatchEffectEntity {
    changed_attributes: Vec<String>,
    expanded: Value,
    types: Vec<String>,
}

struct IndexedBatchEffectEntity {
    changed_attributes: Vec<String>,
    payload_index: usize,
    types: Vec<String>,
}

/// Build a [`PreparedEntityRow`] from an already-merged expanded payload.
///
/// Used by update/merge/upsert flows where the caller has already produced the
/// final merged payload and needs a row for `save_batch`.
fn row_from_expanded(id: String, expanded: Value) -> (PreparedEntityRow, Vec<String>) {
    let types = expanded::extract_types(&expanded);
    let scope_paths_json = Value::Array(
        expanded::extract_scopes(&expanded)
            .into_iter()
            .map(|s| Value::String(sql::scope_to_ltree(&s)))
            .collect(),
    );
    let location = expanded::extract_location_wkb(&expanded);
    let observation_space = expanded::extract_observation_space_wkb(&expanded);
    let operation_space = expanded::extract_operation_space_wkb(&expanded);
    let expires_at = expanded::extract_expires_at(&expanded);

    (
        PreparedEntityRow {
            id,
            types_json: Value::Array(types.iter().cloned().map(Value::String).collect()),
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

    const fn prepare_execution(&self, len: usize) -> PrepareExecution {
        if len >= self.batch_prepare_blocking_threshold {
            PrepareExecution::Offload
        } else {
            PrepareExecution::Inline
        }
    }

    fn build_prepared_entity(id: String, expanded: Value) -> Result<PreparedEntity, (String, AndromedaError)> {
        validate_attribute_envelopes(&expanded).map_err(|err| (id.clone(), err))?;
        let changed_attributes = extract_attribute_keys(&expanded);
        let types = extract_entity_types(&expanded);
        let scopes = extract_scopes(&expanded);
        let location = expanded::extract_location_wkb(&expanded);
        let observation_space = expanded::extract_observation_space_wkb(&expanded);
        let operation_space = expanded::extract_operation_space_wkb(&expanded);
        let expires_at = expanded::extract_expires_at(&expanded);

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

    /// Serialize, normalize, expand, and validate a single entity, returning the
    /// pre-processed data needed for bulk SQL operations.
    ///
    /// On failure, returns the entity ID paired with the error so the caller
    /// can aggregate per-entity errors.
    async fn prepare_entity(&self, entity: &Entity, context: &ContextSpec, execution: PrepareExecution) -> Result<PreparedEntity, (String, AndromedaError)> {
        let id = entity.id.to_string();

        let mut entity_json =
            serde_json::to_value(entity).map_err(|err| (id.clone(), AndromedaError::entity_validation(format!("Invalid entity payload: {}", err))))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await.map_err(|err| {
            let err: AndromedaError = err.into();
            (id.clone(), err)
        })?;

        match execution {
            PrepareExecution::Inline => Self::build_prepared_entity(id, expanded),
            PrepareExecution::Offload => task::spawn_blocking(move || Self::build_prepared_entity(id, expanded)).await.map_err(|error| {
                (
                    "unknown".to_string(),
                    AndromedaError::internal(format!("Batch preparation task failed: {error}")),
                )
            })?,
        }
    }

    async fn prepare_entity_refs(&self, entities: &[&Entity], context: &ContextSpec) -> (Vec<PreparedEntity>, Vec<(String, AndromedaError)>) {
        let execution = self.prepare_execution(entities.len());
        let results: Vec<_> = stream::iter(entities.iter().copied())
            .map(|entity| async move { self.prepare_entity(entity, context, execution).await })
            .buffered(self.batch_concurrency)
            .collect()
            .await;

        let mut prepared = Vec::with_capacity(results.len());
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(entity) => prepared.push(entity),
                Err(error) => errors.push(error),
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

            temporal_entities.push((id.clone(), effect.expanded));
            match id.parse::<Uri>() {
                Ok(entity_uri) => {
                    change_events.push(ChangeEvent::new(entity_uri, effect.types, trigger_reason).with_changed_attributes(effect.changed_attributes));
                }
                Err(_error) => {
                    tracing::error!(entity_id = %id, event = "entity_uri_parse_failed");
                }
            }
            success.push(id.clone());
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

            temporal_entities.push((id.clone(), expanded));
            match id.parse::<Uri>() {
                Ok(entity_uri) => {
                    change_events.push(ChangeEvent::new(entity_uri, effect.types, trigger_reason).with_changed_attributes(effect.changed_attributes));
                }
                Err(_error) => {
                    tracing::error!(entity_id = %id, event = "entity_uri_parse_failed");
                }
            }
            success.push(id.clone());
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

            let trigger_reason = if was_created {
                created.push(id.clone());
                TriggerReason::EntityCreated
            } else {
                TriggerReason::EntityUpdated
            };
            temporal_entities.push((id.clone(), expanded));
            match id.parse::<Uri>() {
                Ok(entity_uri) => {
                    change_events.push(ChangeEvent::new(entity_uri, effect.types, trigger_reason).with_changed_attributes(effect.changed_attributes));
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

    async fn batch_create_chunk(&self, entities: &[&Entity], context: &ContextSpec, original_batch_size: usize) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entity_refs(entities, context).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

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
                    columns.types_json.push(Value::Array(types.iter().cloned().map(Value::String).collect()));
                    columns
                        .scope_paths_json
                        .push(Value::Array(scopes.into_iter().map(|s| Value::String(sql::scope_to_ltree(&s))).collect()));
                    columns.payloads.push(expanded);
                    columns.locations.push(location);
                    columns.obs_spaces.push(observation_space);
                    columns.op_spaces.push(operation_space);
                    columns.expires_ats.push(expires_at);
                    entry.insert(IndexedBatchEffectEntity {
                        changed_attributes,
                        payload_index,
                        types,
                    });
                }
            }
        }

        let inserted_ids = self.repo.create_batch(&columns).await?;
        let inserted_set: HashSet<&str> = inserted_ids.iter().map(String::as_str).collect();
        for id in entity_data.keys() {
            if !inserted_set.contains(id.as_str()) {
                errors.push((id.clone(), AndromedaError::entity_already_exists(id)));
            }
        }

        let mut payloads = columns.payloads.into_iter().map(Some).collect::<Vec<_>>();
        let (job, success) = Self::drain_indexed_effect_entities(&mut entity_data, &mut payloads, &inserted_ids, TriggerReason::EntityCreated);
        self.dispatch_batch_side_effects(original_batch_size, job).await;

        Ok(BatchResult { success, errors })
    }

    async fn batch_update_chunk(
        &self,
        entities: &[Entity],
        context: &ContextSpec,
        no_overwrite: bool,
        original_batch_size: usize,
    ) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<String> = prepared.iter().map(|entity| entity.id.clone()).collect();
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
            if no_overwrite {
                let skipped = has_overlapping_attrs(&existing, attrs);
                if let Err(message) = append_entity_attrs(&mut existing, attrs, true) {
                    errors.push((id, AndromedaError::internal(message)));
                    continue;
                }
                if skipped {
                    errors.push((
                        id.clone(),
                        AndromedaError::entity_validation("Some attributes already exist and were not updated due to noOverwrite"),
                    ));
                }
            } else {
                if let Err(message) = update_entity_attrs(&mut existing, attrs) {
                    errors.push((id, AndromedaError::internal(message)));
                    continue;
                }
                if let Err(message) = append_entity_attrs(&mut existing, attrs, false) {
                    errors.push((id, AndromedaError::internal(message)));
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

        let save_columns = decompose_rows(save_rows);
        let saved_ids = self.repo.save_batch(&save_columns).await?;
        let (job, success) = Self::drain_effect_entities(&mut effect_entities, &saved_ids, TriggerReason::EntityUpdated);
        self.dispatch_batch_side_effects(original_batch_size, job).await;

        Ok(BatchResult { success, errors })
    }

    async fn batch_merge_chunk(&self, entities: &[Entity], context: &ContextSpec, original_batch_size: usize) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;
        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<String> = prepared.iter().map(|entity| entity.id.clone()).collect();
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut effect_entities = HashMap::with_capacity(prepared.len());
        let mut save_rows = Vec::with_capacity(prepared.len());

        for prepared_entity in prepared {
            let id = prepared_entity.id;
            let Some(mut existing) = existing_map.get(&id).cloned() else {
                errors.push((id.clone(), AndromedaError::entity_not_found(&id)));
                continue;
            };

            if let Err(message) = merge_entity(&mut existing, &prepared_entity.expanded) {
                errors.push((id, AndromedaError::internal(message)));
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

        let save_columns = decompose_rows(save_rows);
        let saved_ids = self.repo.save_batch(&save_columns).await?;
        let (job, success) = Self::drain_effect_entities(&mut effect_entities, &saved_ids, TriggerReason::EntityUpdated);
        self.dispatch_batch_side_effects(original_batch_size, job).await;

        Ok(BatchResult { success, errors })
    }

    async fn batch_upsert_replace_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(String, AndromedaError)>,
        original_batch_size: usize,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
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
            let types_json = Value::Array(types.iter().cloned().map(Value::String).collect());
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
            columns.payloads.push(expanded);
            columns.locations.push(location);
            columns.obs_spaces.push(observation_space);
            columns.op_spaces.push(operation_space);
            columns.expires_ats.push(expires_at);
        }

        let upserted = self.repo.upsert_batch(&columns).await?;
        let mut payloads = columns.payloads.into_iter().map(Some).collect::<Vec<_>>();
        let (job, success, created) = Self::drain_upsert_indexed_effect_entities(&mut entity_data, &mut payloads, upserted);
        self.dispatch_batch_side_effects(original_batch_size, job).await;

        Ok((BatchResult { success, errors: Vec::new() }, created))
    }

    async fn batch_upsert_update_chunk(
        &self,
        entities: &[&Entity],
        context: &ContextSpec,
        errors: &mut Vec<(String, AndromedaError)>,
        original_batch_size: usize,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
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
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut created_effects = HashMap::new();
        let mut created_columns = PreparedEntityColumns::with_capacity(prepared.len());
        let mut updated_effects = HashMap::new();
        let mut updated_rows = Vec::new();

        for prepared_entity in prepared {
            let id = prepared_entity.id.clone();
            if let Some(existing) = existing_map.get(&id) {
                let mut merged = existing.clone();
                if let Err(message) = merge_entity(&mut merged, &prepared_entity.expanded) {
                    errors.push((id, AndromedaError::internal(message)));
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
                let types_json = Value::Array(types.iter().cloned().map(Value::String).collect());
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
                created_columns.payloads.push(expanded);
                created_columns.locations.push(location);
                created_columns.obs_spaces.push(observation_space);
                created_columns.op_spaces.push(operation_space);
                created_columns.expires_ats.push(expires_at);
            }
        }

        let created_ids = if created_columns.ids.is_empty() {
            Vec::new()
        } else {
            self.repo.create_batch(&created_columns).await?
        };
        let updated_ids = if updated_rows.is_empty() {
            Vec::new()
        } else {
            let updated_columns = decompose_rows(updated_rows);
            self.repo.save_batch(&updated_columns).await?
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
        self.dispatch_batch_side_effects(
            original_batch_size,
            BatchSideEffectJob {
                temporal_entities,
                change_events,
            },
        )
        .await;

        Ok((BatchResult { success, errors: Vec::new() }, created))
    }

    // -------------------------------------------------------------------------
    // Batch create (NGSI-LD 5.6.7)
    // -------------------------------------------------------------------------

    /// Create multiple entities in a single batch operation (NGSI-LD 5.6.7).
    pub async fn batch_create(&self, entities: &[Entity], context: &ContextSpec) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let (deduped, mut errors) = Self::dedupe_create_entities(entities);
        if deduped.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let chunk_len = self.batch_chunk_len(deduped.len());
        let batch_size = entities.len();
        let chunk_results: Vec<Result<BatchResult, AndromedaError>> = stream::iter(deduped.chunks(chunk_len))
            .map(|chunk| async move { self.batch_create_chunk(chunk, context, batch_size).await })
            .buffered(self.batch_chunk_concurrency)
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
    pub async fn batch_delete(&self, entity_ids: &[String]) -> Result<BatchResult, AndromedaError> {
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
        let chunk_results: Vec<Result<(Vec<String>, Vec<(String, AndromedaError)>), AndromedaError>> = stream::iter(unique_ids.chunks(chunk_len))
            .map(|chunk| async move { self.batch_delete_chunk(chunk, batch_size).await })
            .buffered(self.batch_chunk_concurrency)
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

    async fn batch_delete_chunk(&self, chunk: &[String], original_batch_size: usize) -> Result<(Vec<String>, Vec<(String, AndromedaError)>), AndromedaError> {
        let deleted = self.repo.delete_batch(chunk).await?;
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
            let entity_types = extract_entity_types(&deleted_entity.payload);
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

        self.dispatch_batch_side_effects(
            original_batch_size,
            BatchSideEffectJob {
                temporal_entities: Vec::new(),
                change_events,
            },
        )
        .await;

        Ok((success, errors))
    }

    // -------------------------------------------------------------------------
    // Batch update (NGSI-LD 5.6.9)
    // -------------------------------------------------------------------------

    /// Update attributes of multiple entities in a single batch operation (NGSI-LD 5.6.9).
    pub async fn batch_update(&self, entities: &[Entity], context: &ContextSpec, no_overwrite: bool) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let chunk_len = self.batch_chunk_len(entities.len());
        let batch_size = entities.len();
        let chunk_results: Vec<Result<BatchResult, AndromedaError>> = stream::iter(entities.chunks(chunk_len))
            .map(|chunk| async move { self.batch_update_chunk(chunk, context, no_overwrite, batch_size).await })
            .buffered(self.batch_chunk_concurrency)
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
    pub async fn batch_merge(&self, entities: &[Entity], context: &ContextSpec) -> Result<BatchResult, AndromedaError> {
        self.batch_activity.record_activity();
        let chunk_len = self.batch_chunk_len(entities.len());
        let batch_size = entities.len();
        let chunk_results: Vec<Result<BatchResult, AndromedaError>> = stream::iter(entities.chunks(chunk_len))
            .map(|chunk| async move { self.batch_merge_chunk(chunk, context, batch_size).await })
            .buffered(self.batch_chunk_concurrency)
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
    pub async fn batch_upsert(&self, entities: &[Entity], context: &ContextSpec, update_mode: bool) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        self.batch_activity.record_activity();
        let deduped = Self::dedupe_upsert_entities(entities);
        let chunk_len = self.batch_chunk_len(deduped.len());
        let batch_size = entities.len();

        type ChunkOutcome = (BatchResult, Vec<(String, AndromedaError)>, Vec<String>);
        let chunk_results: Vec<Result<ChunkOutcome, AndromedaError>> = stream::iter(deduped.chunks(chunk_len))
            .map(|chunk| async move {
                let mut chunk_errors = Vec::new();
                let outcome = if update_mode {
                    self.batch_upsert_update_chunk(chunk, context, &mut chunk_errors, batch_size).await
                } else {
                    self.batch_upsert_replace_chunk(chunk, context, &mut chunk_errors, batch_size).await
                };
                outcome.map(|(result, created)| (result, chunk_errors, created))
            })
            .buffered(self.batch_chunk_concurrency)
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
