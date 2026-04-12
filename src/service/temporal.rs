//! Service layer for the NGSI-LD Temporal API.

use crate::{
    core::error::AndromedaError,
    domain::temporal::TemporalFormat,
    http::parameter::query::{SharedQueryParams, temporal::TemporalQueryParameters},
    i18n::collation::augment_expanded_with_collation_metadata,
    jsonld::{CompactOptions, ContextSpec, JsonLdEngine, vocab},
    query::{
        compiled::{CompiledTemporalQuery, GeoQuery, IdPattern, LastNOrigin, ScopeQuery},
        sql::temporal::{build_stage1_entity_ids, build_temporal_history_query},
    },
    renderer::{
        SysAttrsInclusion,
        temporal::{
            build_content_range,
            build_entity_temporal,
            build_entity_temporal_aggregated_data,
            ensure_temporal_attrs_are_arrays,
            transform_to_temporal_values,
        },
    },
    repository::{
        entity::EntityRepository,
        temporal::{DeleteScope, TemporalRepository},
    },
    service::{
        context_source_forwarding::{ForwardedEntities, merge_temporal_query_results},
        entity::extract_entity_types,
        temporal_recorder::TemporalRecorder,
    },
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::Row;
use std::{borrow::Cow, sync::Arc};

/// Format a DateTime<Utc> as ISO 8601 with second precision.
pub(crate) fn format_datetime(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Service layer for NGSI-LD Temporal Entity operations (read and write).
pub struct TemporalService {
    repo: Arc<TemporalRepository>,
    recorder: TemporalRecorder,
    entity_repo: EntityRepository,
    engine: JsonLdEngine,
    default_last_n: u32,
}

/// Result of create_or_update operation.
#[derive(Debug)]
pub enum CreateOrUpdateResult {
    /// A new temporal entity was created, carrying the assigned entity ID.
    Created(String),
    /// Attributes were appended to an existing temporal entity.
    Updated,
}

/// Result of a temporal read operation.
pub struct TemporalReadResult {
    /// The compacted temporal entity JSON payload.
    pub value: Value,
    /// Pre-built Content-Range header value, present when system truncation occurred.
    pub content_range: Option<String>,
}

/// Warning collected when a per-entity temporal query step fails non-fatally.
#[derive(Debug)]
pub struct TemporalQueryWarning {
    /// The entity that triggered the warning.
    pub entity_id: String,
    /// Human-readable description of the problem.
    pub message: String,
}

/// Result of a temporal query operation.
pub struct TemporalQueryResult {
    /// Temporal entity representations matching the query.
    pub entities: Vec<Value>,
    /// Total entity count when the `count` option was requested.
    pub total_count: Option<i64>,
    /// Whether any entity's history was truncated by the system lastN cap.
    pub any_truncated: bool,
    /// Pre-built Content-Range header value when truncation occurred.
    pub content_range: Option<String>,
    /// Non-fatal warnings encountered during per-entity processing.
    pub warnings: Vec<TemporalQueryWarning>,
}

impl TemporalService {
    /// Create a new temporal service with the given connection pool, JSON-LD engine, and default lastN cap.
    pub fn new(pool: &sqlx::postgres::PgPool, engine: JsonLdEngine, default_last_n: u32) -> TemporalService {
        let repo = Arc::new(TemporalRepository::new(pool));
        let recorder = TemporalRecorder::new(repo.clone()); // clone: Arc ref-count bump; repo is shared between service and recorder
        let entity_repo = EntityRepository::new(pool);

        TemporalService {
            repo,
            recorder,
            entity_repo,
            engine,
            default_last_n,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    // -------------------------------------------------------------------------
    // Write operations
    // -------------------------------------------------------------------------

    /// Create a new temporal entity or append attributes to an existing one.
    pub async fn create_or_update(&self, payload: Value, context: &ContextSpec) -> Result<CreateOrUpdateResult, AndromedaError> {
        let mut expanded = self.engine.expand(payload.clone(), context).await?; // clone: expand() consumes the Value; payload is still used below for type extraction
        augment_expanded_with_collation_metadata(&mut expanded);
        let entity_obj = expanded
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .ok_or_else(|| AndromedaError::temporal_validation("Entity must be a JSON object"))?;

        let entity_id = entity_obj
            .get("@id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AndromedaError::temporal_validation("Entity missing @id"))?;

        if entity_id.is_empty() || url::Url::parse(entity_id).is_err() {
            return Err(AndromedaError::temporal_validation("Invalid entity id"));
        }

        // Note: Per NGSI-LD spec, temporal entities can be created independently
        // of the main entity table. We do NOT require the entity to exist first.

        let exists = self.repo.tea_exists_for_entity(entity_id).await?;

        if exists {
            self.add_attrs_internal(entity_id, &expanded).await?;
            Ok(CreateOrUpdateResult::Updated)
        } else {
            // Create base entity placeholder if it doesn't exist in the entities table yet,
            // because temporal_entity_attributes has a FOREIGN KEY constraint to entities(id).
            let base_exists = self.entity_repo.exists(entity_id).await?;
            if !base_exists {
                let mut types = Vec::new();
                if let Some(type_arr) = entity_obj.get("@type").and_then(|v| v.as_array()) {
                    for t in type_arr {
                        if let Some(t_str) = t.as_str() {
                            types.push(t_str);
                        }
                    }
                }
                // Fallback to "Entity" if type is missing, though NGSI-LD spec requires one
                if types.is_empty() {
                    types.push("https://uri.etsi.org/ngsi-ld/Entity");
                }

                // Save initial base entity using the expanded payload so that
                // extract_entity_types() can find @type in the stored JSON.
                let expires_at = crate::domain::expanded::extract_expires_at(&expanded);
                let _ = self.entity_repo.create(entity_id, &types, &[], &expanded, expires_at).await;
            }

            self.recorder.record_entity_attributes(entity_id, &expanded).await?;
            Ok(CreateOrUpdateResult::Created(entity_id.to_string()))
        }
    }

    /// Add temporal attributes to an existing temporal entity.
    pub async fn add_attrs(&self, entity_id: &str, payload: Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if !self.repo.tea_exists_for_entity(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }

        let mut expanded = self.engine.expand(payload, context).await?;
        augment_expanded_with_collation_metadata(&mut expanded);
        self.add_attrs_internal(entity_id, &expanded).await?;
        Ok(())
    }

    async fn add_attrs_internal(&self, entity_id: &str, expanded: &Value) -> Result<(), AndromedaError> {
        self.recorder.record_entity_attributes(entity_id, expanded).await
    }

    /// Delete all temporal data for an entity.
    pub async fn delete(&self, entity_id: &str) -> Result<(), AndromedaError> {
        if !self.repo.tea_exists_for_entity(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }
        self.repo.delete_teas_for_entity(entity_id).await?;
        Ok(())
    }

    /// Delete a specific temporal attribute (optionally filtered by dataset ID).
    pub async fn delete_attr(
        &self,
        entity_id: &str,
        attr_id: &str,
        dataset_id: Option<&str>,
        scope: DeleteScope,
        context: &ContextSpec,
    ) -> Result<(), AndromedaError> {
        let attr_iri = self.engine.expand_term(attr_id, context).await?;

        if !self.repo.tea_exists_for_entity(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }

        let teas = self.repo.get_teas_for_entity_attr(entity_id, &attr_iri).await?;
        if teas.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute {} not found on entity {}",
                attr_iri, entity_id
            )));
        }

        self.repo.delete_tea(entity_id, &attr_iri, dataset_id, scope).await?;
        Ok(())
    }

    /// Replace an existing attribute instance with new data.
    pub async fn modify_attr_instance(
        &self,
        entity_id: &str,
        attr_id: &str,
        instance_id: &str,
        payload: Value,
        context: &ContextSpec,
    ) -> Result<(), AndromedaError> {
        let attr_iri = self.engine.expand_term(attr_id, context).await?;

        if !self.repo.tea_exists_for_entity(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(format!("Entity {} not found", entity_id)));
        }

        let teas = self.repo.get_teas_for_entity_attr(entity_id, &attr_iri).await?;
        if teas.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute {} not found on entity {}",
                attr_iri, entity_id
            )));
        }

        if self.repo.get_instance(instance_id, entity_id, &attr_iri).await?.is_none() {
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {} not found", instance_id)));
        }

        let mut expanded = self.engine.expand(payload, context).await?;
        augment_expanded_with_collation_metadata(&mut expanded);
        let instance_value = expanded.as_array().and_then(|arr| arr.first()).cloned().unwrap_or(expanded); // clone: extracting first element from array requires owned value; original consumed by unwrap_or

        self.recorder
            .update_attribute_instance(entity_id, &attr_iri, instance_id, &instance_value)
            .await
    }

    /// Delete a specific attribute instance by its instance ID.
    pub async fn delete_attr_instance(&self, entity_id: &str, attr_id: &str, instance_id: &str, context: &ContextSpec) -> Result<(), AndromedaError> {
        let attr_iri = self.engine.expand_term(attr_id, context).await?;

        if !self.repo.tea_exists_for_entity(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(format!("Entity {} not found", entity_id)));
        }

        let teas = self.repo.get_teas_for_entity_attr(entity_id, &attr_iri).await?;
        if teas.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute {} not found on entity {}",
                attr_iri, entity_id
            )));
        }

        if self.repo.get_instance(instance_id, entity_id, &attr_iri).await?.is_none() {
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {} not found", instance_id)));
        }

        self.repo.delete_instance(instance_id, entity_id, &attr_iri).await
    }

    // -------------------------------------------------------------------------
    // Read operations
    // -------------------------------------------------------------------------

    /// Retrieve the temporal representation of a single entity.
    pub async fn get(&self, entity_id: &str, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalReadResult, AndromedaError> {
        params.validate(false)?;

        if !self.repo.tea_exists_for_entity(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }

        let entity_types = if let Some(entity_payload) = self.entity_repo.get(entity_id).await? {
            extract_entity_types(&entity_payload)
        } else {
            Vec::new()
        };

        let compiled = self.compile_temporal_query(params, context).await?;

        let mut binds = vec![crate::query::sql::BindValue::String(Cow::Owned(entity_id.to_string()))];
        let mut param_idx: usize = 2;
        let stage2 = build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx)?;

        let rows = self.repo.execute_temporal_query(&stage2.sql, &stage2.bind_values).await?;

        // When an attrs filter is specified and nothing matches, return 404
        if rows.is_empty() && !compiled.attr_filter.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "No temporal data found for the specified attributes on entity {}",
                entity_id
            )));
        }

        let format = compiled.format;

        if format == TemporalFormat::AggregatedValues {
            let result = self.build_aggregated_compacted(entity_id, &entity_types, &rows, &compiled, context).await?;
            return Ok(TemporalReadResult {
                value: result,
                content_range: None,
            });
        }

        let result = build_entity_temporal(entity_id, &entity_types, &rows, &compiled)?;
        let mut compacted = self.engine.compact(result.entity, context, CompactOptions::default()).await?;
        ensure_temporal_attrs_are_arrays(&mut compacted);

        let value = if format == TemporalFormat::TemporalValues {
            transform_to_temporal_values(compacted)
        } else {
            compacted
        };

        let content_range = if result.truncated {
            build_content_range(&compiled, result.time_range, result.max_partition_total, compiled.last_n_origin)
        } else {
            None
        };

        Ok(TemporalReadResult { value, content_range })
    }

    /// Query temporal entities matching the given filter and temporal parameters.
    pub async fn query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalQueryResult, AndromedaError> {
        params.validate(true)?;

        let compiled = self.compile_temporal_query(params, context).await?;

        let mut binds = Vec::new();
        let mut param_idx = 1;
        let stage1 = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx)?;

        let entity_id_rows = self.repo.execute_temporal_query(&stage1.sql, &stage1.bind_values).await?;

        let entity_ids: Vec<String> = entity_id_rows.iter().filter_map(|r| r.try_get("entity_id").ok()).collect();

        let total_count = if compiled.count {
            let count_sql = stage1.count_sql.as_ref().ok_or_else(|| AndromedaError::internal("Count SQL not built"))?;
            let count_rows = self.repo.execute_temporal_query(count_sql, &stage1.bind_values).await?;
            count_rows.first().and_then(|r| r.try_get::<i64, _>("count").ok())
        } else {
            None
        };

        // Batch-fetch entity payloads to extract types (avoids N+1 queries)
        let entity_payloads = self.entity_repo.get_batch(&entity_ids).await?;
        let entity_types_map: std::collections::HashMap<String, Vec<String>> = entity_payloads
            .iter()
            .map(|(id, payload)| (id.clone(), extract_entity_types(payload))) // clone: owned key for HashMap entry
            .collect();

        let mut entities = Vec::new();
        let mut warnings = Vec::new();
        let mut any_truncated = false;
        let mut overall_min: Option<DateTime<Utc>> = None;
        let mut overall_max: Option<DateTime<Utc>> = None;
        let mut overall_partition_total: Option<i64> = None;
        let is_aggregated = compiled.format == TemporalFormat::AggregatedValues;
        for entity_id in entity_ids {
            let entity_types = entity_types_map.get(&entity_id).cloned().unwrap_or_default(); // clone: owned copy needed; map is borrowed while loop continues
            let mut binds = vec![crate::query::sql::BindValue::String(Cow::Owned(entity_id.clone()))]; // clone: entity_id used later in warnings and result building
            let mut param_idx = 2;
            let stage2 = match build_temporal_history_query(1, &compiled, &mut binds, &mut param_idx) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, entity_id = %entity_id, "Failed to build temporal history query");
                    warnings.push(TemporalQueryWarning {
                        entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let rows = match self.repo.execute_temporal_query(&stage2.sql, &stage2.bind_values).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, entity_id = %entity_id, "Failed to execute temporal query");
                    warnings.push(TemporalQueryWarning {
                        entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            if !rows.is_empty() {
                if is_aggregated {
                    match self.build_aggregated_compacted(&entity_id, &entity_types, &rows, &compiled, context).await {
                        Ok(result) => entities.push(result),
                        Err(e) => {
                            tracing::warn!(error = %e, entity_id = %entity_id, "Failed to build aggregated temporal result");
                            warnings.push(TemporalQueryWarning {
                                entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                                message: e.to_string(),
                            });
                        }
                    }
                } else {
                    match build_entity_temporal(&entity_id, &entity_types, &rows, &compiled) {
                        Ok(result) => {
                            if result.truncated {
                                any_truncated = true;
                            }
                            if let Some((min_t, max_t)) = result.time_range {
                                overall_min = Some(overall_min.map_or(min_t, |m: DateTime<Utc>| m.min(min_t)));
                                overall_max = Some(overall_max.map_or(max_t, |m: DateTime<Utc>| m.max(max_t)));
                            }
                            if let Some(pt) = result.max_partition_total {
                                overall_partition_total = Some(overall_partition_total.map_or(pt, |m: i64| m.max(pt)));
                            }
                            match self.engine.compact(result.entity, context, CompactOptions::default()).await {
                                Ok(mut compacted) => {
                                    ensure_temporal_attrs_are_arrays(&mut compacted);
                                    let result = if compiled.format == TemporalFormat::TemporalValues {
                                        transform_to_temporal_values(compacted)
                                    } else {
                                        compacted
                                    };
                                    entities.push(result);
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, entity_id = %entity_id, "Failed to compact temporal entity");
                                    warnings.push(TemporalQueryWarning {
                                        entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                                        message: e.to_string(),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, entity_id = %entity_id, "Failed to build temporal entity response");
                            warnings.push(TemporalQueryWarning {
                                entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                                message: e.to_string(),
                            });
                        }
                    }
                }
            }
        }

        let content_range = if any_truncated {
            build_content_range(&compiled, overall_min.zip(overall_max), overall_partition_total, compiled.last_n_origin)
        } else {
            None
        };

        Ok(TemporalQueryResult {
            entities,
            total_count,
            any_truncated,
            content_range,
            warnings,
        })
    }

    /// Build an aggregated temporal response in compacted form, bypassing JSON-LD compaction.
    /// Aggregation keys (avg, min, etc.) aren't valid JSON-LD IRIs and would be dropped by compact().
    async fn build_aggregated_compacted(
        &self,
        entity_id: &str,
        entity_types: &[String],
        rows: &[sqlx::postgres::PgRow],
        compiled: &CompiledTemporalQuery,
        context: &ContextSpec,
    ) -> Result<Value, AndromedaError> {
        // Build aggregation data keyed by attribute IRI
        let aggr_data = build_entity_temporal_aggregated_data(rows, compiled)?;

        // Compact each attribute IRI individually by building a 1-key skeleton and compacting it
        let mut iri_to_short: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for attr_iri in aggr_data.keys() {
            let mut skeleton = serde_json::Map::new();
            skeleton.insert("@id".to_string(), json!(entity_id));
            skeleton.insert(attr_iri.clone(), json!([{"@type": [vocab::PROPERTY]}])); // clone: owned key for JSON map insertion
            if let Ok(compacted) = self.engine.compact(Value::Object(skeleton), context, CompactOptions::default()).await
                && let Some(obj) = compacted.as_object()
            {
                for key in obj.keys() {
                    if !["id", "type", "@context"].contains(&key.as_str()) {
                        iri_to_short.insert(attr_iri.clone(), key.clone()); // clone: owned keys for HashMap entry; originals are borrowed
                        break;
                    }
                }
            }
        }

        // Compact entity type by compacting a minimal type skeleton
        let type_skeleton = json!({"@id": entity_id, "@type": entity_types});
        let compacted_type = if let Ok(compacted) = self.engine.compact(type_skeleton, context, CompactOptions::default()).await {
            compacted.as_object().and_then(|o| o.get("type")).cloned().unwrap_or(json!(entity_types)) // clone: need owned Value from borrowed compacted object
        } else {
            json!(entity_types)
        };

        // Build the final result
        let mut result = serde_json::Map::new();
        result.insert("id".to_string(), json!(entity_id));
        result.insert("type".to_string(), compacted_type);

        for (attr_iri, attr_obj) in aggr_data {
            let short_name = iri_to_short.get(&attr_iri).cloned().unwrap_or(attr_iri); // clone: need owned String for map insertion; attr_iri consumed by unwrap_or
            result.insert(short_name, attr_obj);
        }

        Ok(Value::Object(result))
    }

    /// Execute stage 1 entity ID resolution without fetching temporal history.
    ///
    /// Used to build EntityMaps that represent the full result set (spec 5.5.14).
    /// Runs only the lightweight stage 1 SQL (entity ID resolution) without fetching
    /// temporal history, ensuring the EntityMap captures ALL matching IDs, not just the
    /// current page.
    pub async fn query_ids(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<Vec<String>, AndromedaError> {
        params.validate(true)?;
        let compiled = self.compile_temporal_query(params, context).await?;
        let mut binds = Vec::new();
        let mut param_idx = 1;
        let stage1 = build_stage1_entity_ids(&compiled, &mut binds, &mut param_idx)?;
        let entity_id_rows = self.repo.execute_temporal_query(&stage1.sql, &stage1.bind_values).await?;
        Ok(entity_id_rows.iter().filter_map(|r| r.try_get("entity_id").ok()).collect())
    }

    /// Merge local temporal query results with forwarded context source results.
    ///
    /// Returns the merged entity values and any warnings generated during forwarding.
    /// When the forwarded list is empty, returns the local entities unchanged.
    pub fn merge_with_forwarded(local_entities: Vec<Value>, forwarded: Vec<ForwardedEntities>) -> Vec<Value> {
        if forwarded.is_empty() {
            local_entities
        } else {
            merge_temporal_query_results(local_entities, forwarded)
        }
    }

    /// Compile raw temporal query parameters into a validated, expanded query structure.
    ///
    /// Expands terms, resolves IRIs, parses temporal/geo/q filters, and computes
    /// effective lastN and pagination values.
    pub async fn compile_temporal_query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<CompiledTemporalQuery, AndromedaError> {
        let id_pattern = params
            .id_pattern
            .as_deref()
            .map(IdPattern::new)
            .transpose()
            .map_err(|e| AndromedaError::temporal_validation(e.to_string()))?;

        let attr_filter = crate::query::compile::expand_attrs(&params.parse_attrs(), &self.engine, context).await?;

        let dataset_id_filter = params.parse_dataset_ids();

        let timerel = params.parse_timerel().transpose()?;

        let q_expr = crate::query::compile::expand_q_expression(params.q.as_deref(), &self.engine, context).await?;

        // Apply expandValues: expand string RHS values for specified attributes (spec temporal params)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref ev) = params.expand_values {
                let ev_attrs: Vec<String> = ev.split(',').map(|s| s.trim().to_string()).collect();
                let ev_iris = crate::query::compile::expand_attrs(&ev_attrs, &self.engine, context).await?;
                let ev_set: std::collections::HashSet<String> = ev_iris.into_iter().collect();
                Some(crate::jsonld::term_resolver::apply_expand_values(&self.engine, q_expr, &ev_set, context).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        // Apply jsonKeys: expand sub-path keys for specified attributes (spec 6.4.3.2)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref jk) = params.json_keys {
                let jk_attrs: Vec<String> = jk.split(',').map(|s| s.trim().to_string()).collect();
                let jk_iris = crate::query::compile::expand_attrs(&jk_attrs, &self.engine, context).await?;
                let jk_set: std::collections::HashSet<String> = jk_iris.into_iter().collect();
                Some(crate::jsonld::term_resolver::apply_json_keys(&self.engine, q_expr, &jk_set, context).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        let geo = self.compile_geo_query(params, context).await?;

        let types = if let Some(raw_types) = params.entity_type.as_deref() {
            Some(crate::query::compile::expand_entity_types(raw_types, &self.engine, context).await?)
        } else {
            None
        };

        let format = params.parse_format();
        let last_n_cap = self.default_last_n;
        let is_aggregated = format == TemporalFormat::AggregatedValues;
        let last_n_origin = if is_aggregated {
            if params.last_n.is_some() {
                LastNOrigin::UserSpecified
            } else {
                LastNOrigin::None
            }
        } else {
            match params.last_n {
                Some(n) if n > last_n_cap => LastNOrigin::UserSpecifiedCapped,
                Some(_) => LastNOrigin::UserSpecified,
                Option::None => LastNOrigin::SystemCapped,
            }
        };
        let effective_last_n = if !is_aggregated {
            Some(params.last_n.map_or(last_n_cap, |n| n.min(last_n_cap)))
        } else {
            params.last_n
        };

        Ok(CompiledTemporalQuery {
            ids: params.parse_ids(),
            id_pattern,
            types,
            q_expr,
            geo,
            timerel,
            time_at: params
                .time_at
                .as_deref()
                .map(|s| {
                    DateTime::parse_from_rfc3339(s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .map_err(|e| AndromedaError::temporal_validation(format!("Invalid timeAt: {}", e)))
                })
                .transpose()?,
            end_time_at: params
                .end_time_at
                .as_deref()
                .map(|s| {
                    DateTime::parse_from_rfc3339(s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .map_err(|e| AndromedaError::temporal_validation(format!("Invalid endTimeAt: {}", e)))
                })
                .transpose()?,
            timeproperty: params.parse_timeproperty(),
            attr_filter,
            dataset_id_filter,
            last_n: effective_last_n,
            last_n_origin,
            format,
            aggr_methods: params.parse_aggr_methods()?,
            aggr_period_duration: params.aggr_period_duration.clone(), // clone: owned value for compiled query struct
            limit: params.effective_limit(),
            offset: params.effective_offset(),
            count: params.count.unwrap_or(false),
            sys_attrs: if params.has_sys_attrs() {
                SysAttrsInclusion::Include
            } else {
                SysAttrsInclusion::Strip
            },
            scope_q: params.scope_q.as_ref().map(|s| ScopeQuery::new(s.clone())), // clone: owned String for ScopeQuery wrapper
        })
    }

    async fn compile_geo_query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<Option<GeoQuery>, AndromedaError> {
        crate::query::compile::geo::compile_geo_query(
            params.georel.as_deref(),
            params.geometry.as_deref(),
            params.coordinates.as_deref(),
            params.geoproperty.as_deref(),
            &self.engine,
            context,
        )
        .await
    }
}
