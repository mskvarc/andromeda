use crate::{
    domain::{
        entity::Entity,
        notification::{ChangeEvent, TriggerReason},
    },
    error::AndromedaError,
    jsonld::ContextSpec,
    service::entity::{EntityService, extract_attribute_keys, normalize_entity_attributes, unwrap_expanded_ref, validate_attribute_envelopes},
};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::collections::HashSet;
use urn::Urn;

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

impl EntityService {
    /// Create multiple entities in a single batch operation (NGSI-LD 5.6.7).
    pub async fn batch_create(&self, entities: &[Entity], context: &ContextSpec) -> Result<BatchResult, AndromedaError> {
        let results: Vec<_> = stream::iter(entities)
            .map(|entity| async {
                let id = entity.id.to_string();
                match self.create(entity, context).await {
                    Ok(_) => Ok(id),
                    Err(err) => Err((id, err)),
                }
            })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        Ok(partition_results(results))
    }

    /// Create or update multiple entities in a single batch operation (NGSI-LD 5.6.8).
    pub async fn batch_upsert(&self, entities: &[Entity], context: &ContextSpec, update_mode: bool) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        // Deduplicate by entity ID, keeping the last occurrence (per NGSI-LD 5.6.8)
        let mut seen = HashSet::new();
        let deduped: Vec<_> = entities
            .iter()
            .rev()
            .filter(|e| seen.insert(e.id.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let results: Vec<_> = stream::iter(deduped)
            .map(|entity| async move { self.upsert_one(entity, context, update_mode).await })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        let mut success = Vec::new();
        let mut errors = Vec::new();
        let mut created = Vec::new();
        for result in results {
            match result {
                Ok((id, was_created)) => {
                    if was_created {
                        created.push(id.clone()); // clone: value also needed for success list below
                    }
                    success.push(id);
                }
                Err(pair) => errors.push(pair),
            }
        }

        Ok((BatchResult { success, errors }, created))
    }

    async fn upsert_one(&self, entity: &Entity, context: &ContextSpec, update_mode: bool) -> Result<(String, bool), (String, AndromedaError)> {
        let id = entity.id.to_string();
        // Single fetch to check existence and get entity for update
        let existing = self.repo.get(&id).await.map_err(|e| (id.clone(), e))?;

        if existing.is_none() {
            self.create(entity, context).await.map_err(|e| (id.clone(), e))?;
            return Ok((id, true));
        }

        let mut entity_json =
            serde_json::to_value(entity).map_err(|err| (id.clone(), AndromedaError::bad_request_data(format!("Invalid entity payload: {}", err))))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await.map_err(|err| {
            let err: AndromedaError = err.into();
            (id.clone(), err)
        })?;

        if update_mode {
            let mut existing = existing.ok_or_else(|| (id.clone(), AndromedaError::internal("Expected existing entity but found none")))?;
            crate::domain::expanded::merge_entity(&mut existing, &expanded).map_err(|msg| (id.clone(), AndromedaError::internal(msg)))?;
            self.repo.save(&id, &existing).await.map_err(|e| (id.clone(), e))?;
        } else {
            self.repo.save(&id, &expanded).await.map_err(|e| (id.clone(), e))?;
        }

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }
        let types = self.fetch_entity_types(&id).await;
        self.emit_change(ChangeEvent::new(id.clone(), types, TriggerReason::EntityUpdated).with_changed_attributes(extract_attribute_keys(&expanded))); // clone: value also returned in Ok tuple below
        Ok((id, false))
    }

    /// Update attributes of multiple entities in a single batch operation (NGSI-LD 5.6.9).
    pub async fn batch_update(&self, entities: &[Entity], context: &ContextSpec, no_overwrite: bool) -> Result<BatchResult, AndromedaError> {
        let results: Vec<_> = stream::iter(entities)
            .map(|entity| async move { self.update_one(entity, context, no_overwrite).await })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        Ok(partition_results(results))
    }

    async fn update_one(&self, entity: &Entity, context: &ContextSpec, no_overwrite: bool) -> Result<String, (String, AndromedaError)> {
        let id = entity.id.to_string();

        let mut entity_json =
            serde_json::to_value(entity).map_err(|err| (id.clone(), AndromedaError::bad_request_data(format!("Invalid entity payload: {}", err))))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await.map_err(|err| {
            let err: AndromedaError = err.into();
            (id.clone(), err)
        })?;

        validate_attribute_envelopes(&expanded).map_err(|err| (id.clone(), err))?;

        let existing = self
            .repo
            .get(&id)
            .await
            .map_err(|e| (id.clone(), e))?
            .ok_or_else(|| (id.clone(), AndromedaError::not_found(&id)))?;

        let attrs = unwrap_expanded_ref(&expanded);

        let mut existing = existing;
        if no_overwrite {
            let skipped = has_overlapping_attrs(&existing, attrs);
            crate::domain::expanded::append_entity_attrs(&mut existing, attrs, true).map_err(|msg| (id.clone(), AndromedaError::internal(msg)))?;
            self.repo.save(&id, &existing).await.map_err(|e| (id.clone(), e))?;
            if skipped {
                return Err((
                    id,
                    AndromedaError::bad_request_data("Some attributes already exist and were not updated due to noOverwrite"),
                ));
            }
        } else {
            crate::domain::expanded::update_entity_attrs(&mut existing, attrs).map_err(|msg| (id.clone(), AndromedaError::internal(msg)))?;
            crate::domain::expanded::append_entity_attrs(&mut existing, attrs, false).map_err(|msg| (id.clone(), AndromedaError::internal(msg)))?;
            self.repo.save(&id, &existing).await.map_err(|e| (id.clone(), e))?;
        }

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }
        let types = self.fetch_entity_types(&id).await;
        self.emit_change(ChangeEvent::new(id.clone(), types, TriggerReason::EntityUpdated).with_changed_attributes(extract_attribute_keys(&expanded))); // clone: value also returned in Ok below
        Ok(id)
    }

    /// Merge partial updates into multiple entities in a single batch operation.
    pub async fn batch_merge(&self, entities: &[Entity], context: &ContextSpec) -> Result<BatchResult, AndromedaError> {
        let results: Vec<_> = stream::iter(entities)
            .map(|entity| async move { self.merge_one(entity, context).await })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        Ok(partition_results(results))
    }

    async fn merge_one(&self, entity: &Entity, context: &ContextSpec) -> Result<String, (String, AndromedaError)> {
        let id = entity.id.to_string();

        let mut entity_json =
            serde_json::to_value(entity).map_err(|err| (id.clone(), AndromedaError::bad_request_data(format!("Invalid entity payload: {}", err))))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await.map_err(|err| {
            let err: AndromedaError = err.into();
            (id.clone(), err)
        })?;

        validate_attribute_envelopes(&expanded).map_err(|err| (id.clone(), err))?;

        let mut existing = self
            .repo
            .get(&id)
            .await
            .map_err(|e| (id.clone(), e))?
            .ok_or_else(|| (id.clone(), AndromedaError::not_found(&id)))?;

        crate::domain::expanded::merge_entity(&mut existing, &expanded).map_err(|msg| (id.clone(), AndromedaError::internal(msg)))?;
        self.repo.save(&id, &existing).await.map_err(|e| (id.clone(), e))?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }
        let types = self.fetch_entity_types(&id).await;
        self.emit_change(ChangeEvent::new(id.clone(), types, TriggerReason::EntityUpdated).with_changed_attributes(extract_attribute_keys(&expanded))); // clone: value also returned in Ok below
        Ok(id)
    }

    /// Delete multiple entities by ID in a single batch operation (NGSI-LD 5.6.10).
    pub async fn batch_delete(&self, entity_ids: &[String]) -> Result<BatchResult, AndromedaError> {
        // Deduplicate IDs: first occurrence is processed, duplicates get ResourceNotFound errors
        let mut seen = HashSet::new();
        let mut unique_ids = Vec::new();
        let mut duplicate_errors = Vec::new();
        for id in entity_ids {
            if !seen.insert(id.as_str()) {
                duplicate_errors.push((id.clone(), AndromedaError::not_found(id)));
            } else {
                unique_ids.push(id);
            }
        }

        let results: Vec<_> = stream::iter(unique_ids)
            .map(|id| async move { self.delete_one(id).await })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        let mut result = partition_results(results);
        result.errors.extend(duplicate_errors);

        Ok(result)
    }

    async fn delete_one(&self, id: &str) -> Result<String, (String, AndromedaError)> {
        let urn: Urn = id
            .parse()
            .map_err(|_| (id.to_string(), AndromedaError::bad_request_data("Invalid entity id")))?;
        self.delete(&urn).await.map_err(|e| (id.to_string(), e))?;
        Ok(id.to_string())
    }
}

/// Partition a list of results into success IDs and error pairs.
fn partition_results(results: Vec<Result<String, (String, AndromedaError)>>) -> BatchResult {
    let mut success = Vec::new();
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok(id) => success.push(id),
            Err(pair) => errors.push(pair),
        }
    }
    BatchResult { success, errors }
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
        attrs_obj.keys().any(|k| !k.starts_with('@') && entity_obj.contains_key(k))
    } else {
        false
    }
}
