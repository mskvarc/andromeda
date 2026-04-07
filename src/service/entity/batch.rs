use crate::{
    domain::{
        entity::Entity,
        expanded,
        notification::{ChangeEvent, SysAttrsSnapshot, TriggerReason},
    },
    error::AndromedaError,
    jsonld::ContextSpec,
    repository::entity::PreparedEntityRow,
    service::entity::{
        EntityService,
        extract_attribute_keys,
        extract_entity_types,
        normalize_entity_attributes,
        unwrap_expanded_ref,
        validate_attribute_envelopes,
    },
};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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
/// (serialization, normalization, JSON-LD expansion, validation).  The
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
    /// GeoJSON string for `location`, if present.
    location: Option<String>,
    /// GeoJSON string for `observationSpace`, if present.
    observation_space: Option<String>,
    /// GeoJSON string for `operationSpace`, if present.
    operation_space: Option<String>,
    /// Entity expiration timestamp (ETSI GS CIM 009 clause 4.22).
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl PreparedEntity {
    /// Convert this prepared entity into a [`PreparedEntityRow`] for repository
    /// bulk SQL binding. Consumes `self` to avoid cloning the payload.
    fn into_row(self) -> PreparedEntityRow {
        PreparedEntityRow {
            id: self.id,
            types_json: Value::Array(self.types.into_iter().map(Value::String).collect()),
            scopes_json: Value::Array(self.scopes.into_iter().map(Value::String).collect()),
            payload: self.expanded,
            location: self.location,
            observation_space: self.observation_space,
            operation_space: self.operation_space,
            expires_at: self.expires_at,
        }
    }
}

/// Build a [`PreparedEntityRow`] from an already-merged expanded payload.
///
/// Used by update/merge/upsert flows where the caller has already produced the
/// final merged payload and needs a row for `save_batch`.
fn row_from_expanded(id: String, expanded: &Value) -> PreparedEntityRow {
    PreparedEntityRow {
        id,
        types_json: Value::Array(expanded::extract_types(expanded).into_iter().map(Value::String).collect()),
        scopes_json: Value::Array(expanded::extract_scopes(expanded).into_iter().map(Value::String).collect()),
        payload: expanded.clone(), // clone: ownership transfer into row for SQL binding
        location: expanded::extract_location_geojson(expanded),
        observation_space: expanded::extract_observation_space_geojson(expanded),
        operation_space: expanded::extract_operation_space_geojson(expanded),
        expires_at: expanded::extract_expires_at(expanded),
    }
}

impl EntityService {
    /// Serialize, normalize, expand, and validate a single entity, returning the
    /// pre-processed data needed for bulk SQL operations.
    ///
    /// On failure, returns the entity ID paired with the error so the caller
    /// can aggregate per-entity errors.
    async fn prepare_entity(&self, entity: &Entity, context: &ContextSpec) -> Result<PreparedEntity, (String, AndromedaError)> {
        let id = entity.id.to_string();

        let mut entity_json =
            serde_json::to_value(entity).map_err(|err| (id.clone(), AndromedaError::bad_request_data(format!("Invalid entity payload: {}", err))))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await.map_err(|err| {
            let err: AndromedaError = err.into();
            (id.clone(), err)
        })?;
        validate_attribute_envelopes(&expanded).map_err(|err| (id.clone(), err))?;

        let types = extract_entity_types(&expanded);
        let scopes = crate::service::entity::extract_scopes(&expanded);
        let location = expanded::extract_location_geojson(&expanded);
        let observation_space = expanded::extract_observation_space_geojson(&expanded);
        let operation_space = expanded::extract_operation_space_geojson(&expanded);
        let expires_at = expanded::extract_expires_at(&expanded);

        Ok(PreparedEntity {
            id,
            expanded,
            types,
            scopes,
            location,
            observation_space,
            operation_space,
            expires_at,
        })
    }

    /// Prepare multiple entities concurrently, partitioning into successes and errors.
    async fn prepare_entities(&self, entities: &[Entity], context: &ContextSpec) -> (Vec<PreparedEntity>, Vec<(String, AndromedaError)>) {
        let results: Vec<_> = stream::iter(entities)
            .map(|entity| async { self.prepare_entity(entity, context).await })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        let mut prepared = Vec::new();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(p) => prepared.push(p),
                Err(pair) => errors.push(pair),
            }
        }
        (prepared, errors)
    }

    // -------------------------------------------------------------------------
    // Batch create (NGSI-LD 5.6.7)
    // -------------------------------------------------------------------------

    /// Create multiple entities in a single batch operation (NGSI-LD 5.6.7).
    ///
    /// Entities are prepared concurrently (JSON-LD expansion is async), then
    /// inserted in a single SQL round-trip.  Duplicate IDs are detected via
    /// `ON CONFLICT DO NOTHING` and reported as `AlreadyExists` errors.
    pub async fn batch_create(&self, entities: &[Entity], context: &ContextSpec) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        // Keep a map of id -> (expanded, types) for post-insert processing
        let mut entity_data: HashMap<String, (Value, Vec<String>)> = HashMap::with_capacity(prepared.len());
        let mut rows = Vec::with_capacity(prepared.len());
        for p in prepared {
            let id = p.id.clone(); // clone: key for lookup map, original moves into row
            let expanded = p.expanded.clone(); // clone: kept for temporal recording + events, original moves into row
            let types = p.types.clone(); // clone: kept for events, original moves into row
            entity_data.insert(id, (expanded, types));
            rows.push(p.into_row());
        }

        let inserted_ids = self.repo.create_batch(&rows).await?;
        let inserted_set: HashSet<&str> = inserted_ids.iter().map(String::as_str).collect();

        // Entities not in the returned set already existed
        for id in entity_data.keys() {
            if !inserted_set.contains(id.as_str()) {
                errors.push((id.clone(), AndromedaError::already_exists(id))); // clone: owned String needed for error, original borrowed from map
            }
        }

        // Post-insert: temporal recording + change events for successes
        let success = self.post_create_events(&inserted_ids, &entity_data).await;

        Ok(BatchResult { success, errors })
    }

    /// Record temporal attributes and emit EntityCreated events for successfully created entities.
    async fn post_create_events(&self, ids: &[String], entity_data: &HashMap<String, (Value, Vec<String>)>) -> Vec<String> {
        let mut success = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some((expanded, types)) = entity_data.get(id) {
                if let Err(e) = self.temporal_recorder.record_entity_attributes(id, expanded).await {
                    tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
                }
                self.emit_change(
                    ChangeEvent::new(id.clone(), types.clone(), TriggerReason::EntityCreated) // clone: owned values for event, originals borrowed from map
                        .with_changed_attributes(extract_attribute_keys(expanded)),
                );
            }
            success.push(id.clone()); // clone: owned String for success list
        }
        success
    }

    // -------------------------------------------------------------------------
    // Batch delete (NGSI-LD 5.6.10)
    // -------------------------------------------------------------------------

    /// Delete multiple entities by ID in a single batch operation (NGSI-LD 5.6.10).
    ///
    /// Deduplicates input IDs (duplicates get `NotFound` errors).  Deletes all
    /// unique IDs in a single SQL round-trip, using the `RETURNING` clause to
    /// obtain payloads and timestamps for change event emission.
    pub async fn batch_delete(&self, entity_ids: &[String]) -> Result<BatchResult, AndromedaError> {
        // Deduplicate IDs: first occurrence is processed, duplicates get ResourceNotFound errors
        let mut seen = HashSet::new();
        let mut unique_ids = Vec::new();
        let mut errors: Vec<(String, AndromedaError)> = Vec::new();
        for id in entity_ids {
            if !seen.insert(id.as_str()) {
                errors.push((id.clone(), AndromedaError::not_found(id))); // clone: owned copies for error list
            } else {
                unique_ids.push(id.clone()); // clone: owned String for SQL binding
            }
        }

        let deleted = self.repo.delete_batch(&unique_ids).await?;
        let deleted_set: HashSet<&str> = deleted.iter().map(|d| d.id.as_str()).collect();

        // IDs not in the returned set were not found
        for id in &unique_ids {
            if !deleted_set.contains(id.as_str()) {
                errors.push((id.clone(), AndromedaError::not_found(id))); // clone: owned String for error
            }
        }

        // Emit change events for each deleted entity
        let mut success = Vec::with_capacity(deleted.len());
        for d in &deleted {
            let entity_types = extract_entity_types(&d.payload);
            let attr_keys = extract_attribute_keys(&d.payload);
            let sys_attrs = SysAttrsSnapshot {
                created_at: d.created_at,
                modified_at: d.modified_at,
                expires_at: d.expires_at,
            };

            // Emit AttributeDeleted for each attribute so attributeDeleted subscriptions fire
            if !attr_keys.is_empty() {
                self.emit_change(
                    ChangeEvent::new(d.id.clone(), entity_types.clone(), TriggerReason::AttributeDeleted) // clone: value also needed by subsequent EntityDeleted emit
                        .with_changed_attributes(attr_keys)
                        .with_snapshot(d.payload.clone()) // clone: value also needed by subsequent EntityDeleted emit
                        .with_previous(d.payload.clone()) // clone: value also needed by subsequent EntityDeleted emit
                        .with_sys_attrs(Some(sys_attrs.clone())), // clone: value also needed by subsequent EntityDeleted emit
                );
            }

            self.emit_change(
                ChangeEvent::new(d.id.clone(), entity_types, TriggerReason::EntityDeleted) // clone: value also needed for success list
                    .with_snapshot(d.payload.clone()) // clone: separate ownership for snapshot and previous fields
                    .with_previous(d.payload.clone()) // clone: separate ownership for snapshot and previous fields
                    .with_sys_attrs(Some(sys_attrs)),
            );

            success.push(d.id.clone()); // clone: owned String for success list
        }

        Ok(BatchResult { success, errors })
    }

    // -------------------------------------------------------------------------
    // Batch update (NGSI-LD 5.6.9)
    // -------------------------------------------------------------------------

    /// Update attributes of multiple entities in a single batch operation (NGSI-LD 5.6.9).
    ///
    /// Entities are prepared concurrently, existing payloads fetched in a single
    /// batch query, attributes merged in Rust, then saved in a single bulk UPDATE.
    pub async fn batch_update(&self, entities: &[Entity], context: &ContextSpec, no_overwrite: bool) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<String> = prepared.iter().map(|p| p.id.clone()).collect(); // clone: owned Strings for batch query
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut save_rows = Vec::new();
        // Track which entities had their expanded payload for temporal recording + events
        let mut entity_expanded: Vec<(String, Value, Vec<String>)> = Vec::new();

        for p in prepared {
            let id = p.id;

            let Some(mut existing) = existing_map.get(&id).cloned() else {
                // clone: owned Value for in-place mutation
                let err = AndromedaError::not_found(&id);
                errors.push((id, err));
                continue;
            };

            let attrs = unwrap_expanded_ref(&p.expanded);

            if no_overwrite {
                let skipped = has_overlapping_attrs(&existing, attrs);
                if let Err(msg) = crate::domain::expanded::append_entity_attrs(&mut existing, attrs, true) {
                    errors.push((id, AndromedaError::internal(msg)));
                    continue;
                }
                if skipped {
                    errors.push((
                        id.clone(), // clone: id also needed below in save_rows + entity_expanded
                        AndromedaError::bad_request_data("Some attributes already exist and were not updated due to noOverwrite"),
                    ));
                }
            } else {
                if let Err(msg) = crate::domain::expanded::update_entity_attrs(&mut existing, attrs) {
                    errors.push((id, AndromedaError::internal(msg)));
                    continue;
                }
                if let Err(msg) = crate::domain::expanded::append_entity_attrs(&mut existing, attrs, false) {
                    errors.push((id, AndromedaError::internal(msg)));
                    continue;
                }
            }

            save_rows.push(row_from_expanded(id.clone(), &existing)); // clone: id also needed for event tracking
            let types = extract_entity_types(&existing);
            entity_expanded.push((id, p.expanded, types));
        }

        let saved_ids = self.repo.save_batch(&save_rows).await?;
        let saved_set: HashSet<&str> = saved_ids.iter().map(String::as_str).collect();

        // Temporal recording + change events for saved entities
        let mut success = Vec::with_capacity(saved_ids.len());
        for (id, expanded, types) in &entity_expanded {
            if !saved_set.contains(id.as_str()) {
                continue;
            }
            if let Err(e) = self.temporal_recorder.record_entity_attributes(id, expanded).await {
                tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
            }
            self.emit_change(
                ChangeEvent::new(id.clone(), types.clone(), TriggerReason::EntityUpdated) // clone: owned values for event
                    .with_changed_attributes(extract_attribute_keys(expanded)),
            );
            success.push(id.clone()); // clone: owned String for success list
        }

        Ok(BatchResult { success, errors })
    }

    // -------------------------------------------------------------------------
    // Batch merge
    // -------------------------------------------------------------------------

    /// Merge partial updates into multiple entities in a single batch operation.
    ///
    /// Entities are prepared concurrently, existing payloads fetched in a single
    /// batch query, deep-merged in Rust, then saved in a single bulk UPDATE.
    pub async fn batch_merge(&self, entities: &[Entity], context: &ContextSpec) -> Result<BatchResult, AndromedaError> {
        let (prepared, mut errors) = self.prepare_entities(entities, context).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let prepared_ids: Vec<String> = prepared.iter().map(|p| p.id.clone()).collect(); // clone: owned Strings for batch query
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut save_rows = Vec::new();
        let mut entity_expanded: Vec<(String, Value, Vec<String>)> = Vec::new();

        for p in prepared {
            let id = p.id;

            let Some(mut existing) = existing_map.get(&id).cloned() else {
                // clone: owned Value for in-place mutation
                let err = AndromedaError::not_found(&id);
                errors.push((id, err));
                continue;
            };

            if let Err(msg) = crate::domain::expanded::merge_entity(&mut existing, &p.expanded) {
                errors.push((id, AndromedaError::internal(msg)));
                continue;
            }

            let types = extract_entity_types(&existing);
            save_rows.push(row_from_expanded(id.clone(), &existing)); // clone: id also needed for event tracking
            entity_expanded.push((id, p.expanded, types));
        }

        let saved_ids = self.repo.save_batch(&save_rows).await?;
        let saved_set: HashSet<&str> = saved_ids.iter().map(String::as_str).collect();

        let mut success = Vec::with_capacity(saved_ids.len());
        for (id, expanded, types) in &entity_expanded {
            if !saved_set.contains(id.as_str()) {
                continue;
            }
            if let Err(e) = self.temporal_recorder.record_entity_attributes(id, expanded).await {
                tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
            }
            self.emit_change(
                ChangeEvent::new(id.clone(), types.clone(), TriggerReason::EntityUpdated) // clone: owned values for event
                    .with_changed_attributes(extract_attribute_keys(expanded)),
            );
            success.push(id.clone()); // clone: owned String for success list
        }

        Ok(BatchResult { success, errors })
    }

    // -------------------------------------------------------------------------
    // Batch upsert (NGSI-LD 5.6.8)
    // -------------------------------------------------------------------------

    /// Create or update multiple entities in a single batch operation (NGSI-LD 5.6.8).
    ///
    /// Two modes:
    /// - **Replace** (`update_mode = false`): uses a single `INSERT ... ON CONFLICT DO UPDATE`
    ///   to insert new entities and fully replace existing ones.
    /// - **Update** (`update_mode = true`): fetches existing entities, merges in Rust, then uses
    ///   `create_batch` for new entities and `save_batch` for merged entities.
    ///
    /// Returns the batch result and a list of IDs for newly created entities.
    pub async fn batch_upsert(&self, entities: &[Entity], context: &ContextSpec, update_mode: bool) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        // Deduplicate by entity ID, keeping the last occurrence (per NGSI-LD 5.6.8)
        let mut seen = HashSet::new();
        let deduped: Vec<&Entity> = entities
            .iter()
            .rev()
            .filter(|e| seen.insert(e.id.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Prepare deduped entities concurrently (cannot use prepare_entities since
        // deduped is Vec<&Entity>, not &[Entity])
        let results: Vec<_> = stream::iter(deduped)
            .map(|entity| async { self.prepare_entity(entity, context).await })
            .buffer_unordered(self.batch_concurrency)
            .collect()
            .await;

        let mut prepared = Vec::new();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(p) => prepared.push(p),
                Err(pair) => errors.push(pair),
            }
        }

        if prepared.is_empty() {
            return Ok((BatchResult { success: Vec::new(), errors }, Vec::new()));
        }

        if update_mode {
            self.batch_upsert_update_mode(prepared, &mut errors).await
        } else {
            self.batch_upsert_replace_mode(prepared, &mut errors).await
        }
    }

    /// Replace mode: single `INSERT ... ON CONFLICT DO UPDATE` for all entities.
    async fn batch_upsert_replace_mode(
        &self,
        prepared: Vec<PreparedEntity>,
        errors: &mut Vec<(String, AndromedaError)>,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        let mut entity_data: HashMap<String, (Value, Vec<String>)> = HashMap::with_capacity(prepared.len());
        let mut rows = Vec::with_capacity(prepared.len());
        for p in prepared {
            let id = p.id.clone(); // clone: key for lookup map, original moves into row
            let expanded = p.expanded.clone(); // clone: kept for temporal recording + events
            let types = p.types.clone(); // clone: kept for events
            entity_data.insert(id, (expanded, types));
            rows.push(p.into_row());
        }

        let upserted = self.repo.upsert_batch(&rows).await?;

        let mut success = Vec::new();
        let mut created = Vec::new();
        for (id, was_created) in &upserted {
            if let Some((expanded, types)) = entity_data.get(id) {
                if let Err(e) = self.temporal_recorder.record_entity_attributes(id, expanded).await {
                    tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
                }
                let trigger = if *was_created {
                    created.push(id.clone()); // clone: owned String for created list
                    TriggerReason::EntityCreated
                } else {
                    TriggerReason::EntityUpdated
                };
                self.emit_change(
                    ChangeEvent::new(id.clone(), types.clone(), trigger) // clone: owned values for event
                        .with_changed_attributes(extract_attribute_keys(expanded)),
                );
            }
            success.push(id.clone()); // clone: owned String for success list
        }

        Ok((
            BatchResult {
                success,
                errors: std::mem::take(errors),
            },
            created,
        ))
    }

    /// Update mode: fetch existing entities, merge in Rust, then create/save separately.
    async fn batch_upsert_update_mode(
        &self,
        prepared: Vec<PreparedEntity>,
        errors: &mut Vec<(String, AndromedaError)>,
    ) -> Result<(BatchResult, Vec<String>), AndromedaError> {
        let prepared_ids: Vec<String> = prepared.iter().map(|p| p.id.clone()).collect(); // clone: owned Strings for batch query
        let existing_map = self.repo.get_batch(&prepared_ids).await?;

        let mut new_rows = Vec::new();
        let mut save_rows = Vec::new();
        let mut new_entity_data: HashMap<String, (Value, Vec<String>)> = HashMap::new();
        let mut updated_entity_data: Vec<(String, Value, Vec<String>)> = Vec::new();

        for p in prepared {
            let id = p.id;

            if let Some(existing) = existing_map.get(&id) {
                // Entity exists: merge and save
                let mut merged = existing.clone(); // clone: owned Value for in-place mutation
                if let Err(msg) = crate::domain::expanded::merge_entity(&mut merged, &p.expanded) {
                    errors.push((id, AndromedaError::internal(msg)));
                    continue;
                }
                let types = extract_entity_types(&merged);
                save_rows.push(row_from_expanded(id.clone(), &merged)); // clone: id also needed for event tracking
                updated_entity_data.push((id, p.expanded, types));
            } else {
                // New entity: create
                let expanded = p.expanded;
                let types = p.types;
                new_rows.push(PreparedEntityRow {
                    id: id.clone(),                                                                     // clone: id also stored in lookup map below
                    types_json: Value::Array(types.iter().map(|t| Value::String(t.clone())).collect()), // clone: owned strings for JSON array
                    scopes_json: Value::Array(p.scopes.into_iter().map(Value::String).collect()),
                    payload: expanded.clone(), // clone: kept for events in new_entity_data
                    location: p.location,
                    observation_space: p.observation_space,
                    operation_space: p.operation_space,
                    expires_at: p.expires_at,
                });
                new_entity_data.insert(id, (expanded, types));
            }
        }

        // Execute both operations
        let created_ids = if new_rows.is_empty() {
            Vec::new()
        } else {
            self.repo.create_batch(&new_rows).await?
        };
        let saved_ids = if save_rows.is_empty() {
            Vec::new()
        } else {
            self.repo.save_batch(&save_rows).await?
        };

        let saved_set: HashSet<&str> = saved_ids.iter().map(String::as_str).collect();

        let mut success = Vec::new();
        let mut created = Vec::new();

        // Events for created entities
        for id in &created_ids {
            if let Some((expanded, types)) = new_entity_data.get(id) {
                if let Err(e) = self.temporal_recorder.record_entity_attributes(id, expanded).await {
                    tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
                }
                self.emit_change(
                    ChangeEvent::new(id.clone(), types.clone(), TriggerReason::EntityCreated) // clone: owned values for event
                        .with_changed_attributes(extract_attribute_keys(expanded)),
                );
            }
            created.push(id.clone()); // clone: owned String for created list
            success.push(id.clone()); // clone: owned String for success list
        }

        // Events for updated entities
        for (id, expanded, types) in &updated_entity_data {
            if !saved_set.contains(id.as_str()) {
                continue;
            }
            if let Err(e) = self.temporal_recorder.record_entity_attributes(id, expanded).await {
                tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
            }
            self.emit_change(
                ChangeEvent::new(id.clone(), types.clone(), TriggerReason::EntityUpdated) // clone: owned values for event
                    .with_changed_attributes(extract_attribute_keys(expanded)),
            );
            success.push(id.clone()); // clone: owned String for success list
        }

        Ok((
            BatchResult {
                success,
                errors: std::mem::take(errors),
            },
            created,
        ))
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
        attrs_obj.keys().any(|k| !k.starts_with('@') && entity_obj.contains_key(k))
    } else {
        false
    }
}
