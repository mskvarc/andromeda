//! Service layer for the NGSI-LD Temporal API.

use crate::{
    core::error::AndromedaError,
    http::response::content_range::build_content_range,
    service::{
        csource_forwarding::{ForwardedTemporalEntities, merge_temporal_query_results},
        entity::{
            atomic_write::AtomicWrite,
            entity_mutation::{EntityMutation, EntityRecord},
            history_record::HistoryRecord,
            query::selector_compilation::compile_entity_selectors,
            store::EntityStore,
            write_coordinator::WriteCoordinator,
        },
    },
    temporal_render::{
        aggregation::TemporalAggregationRenderer,
        array_normalizer::TemporalArrayNormalizer,
        entity::TemporalEntityRenderer,
        values::TemporalValuesRenderer,
    },
};
use andromeda_jsonld::{
    compaction::compact_attr_iri,
    context_spec::ContextSpec,
    engine::{options::CompactOptions, processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_model::expanded::entity_member::extract_types;
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    entity_selection::EntitySelection,
    error::QueryError,
    filter::{
        expansion::{apply_expand_values, apply_json_keys, expand_q_expr},
        path::expanded_attribute_name,
    },
    geo::{compile as geo_compile, query::GeoQuery},
    pagination::{CountPolicy, Pagination},
    projection::expansion::expand_attrs,
    temporal::{
        compiled::CompiledTemporalQuery,
        instance_selection::InstanceSelection,
        last_n::LastNOrigin,
        representation::Representation,
        window::{TemporalWindow, TimeInterval},
    },
};
use andromeda_ngsi_ld_representation::{normalization::normalize_entity_attributes, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, temporal::TemporalQueryParameters};
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
use andromeda_postgresql_temporal_store::{
    params::DeleteScope,
    recorder::TemporalRecorder,
    store::{TemporalAggregationRow, TemporalHistoryReader, TemporalMutator},
};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

/// Return short IRI token from fragment or last path segment.
fn iri_short_name(iri: &str) -> Option<String> {
    if let Some(pos) = iri.rfind('#') {
        return Some(iri[pos + 1..].to_string());
    }
    iri.rfind('/').map(|pos| iri[pos + 1..].to_string())
}

/// Return whether a context spec contains explicit (client-provided) context.
fn has_explicit_context(context: &ContextSpec) -> bool {
    !context.link_contexts().is_empty() || context.body_context().is_some()
}

/// Normalize compacted temporal payload when no explicit context was provided.
///
/// This keeps top-level keys and `type` values readable by collapsing expanded IRIs
/// to their fragment/last-path short names.
fn normalize_temporal_payload_without_context(value: &mut Value) {
    let reserved: HashSet<&str> = ["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"].into_iter().collect();

    let Some(obj) = value.as_object_mut() else {
        return;
    };

    if let Some(type_value) = obj.get_mut("type") {
        match type_value {
            Value::String(type_name) => {
                if type_name.starts_with("http")
                    && let Some(short) = iri_short_name(type_name)
                {
                    *type_name = short;
                }
            }
            Value::Array(types) => {
                for t in types {
                    if let Some(type_name) = t.as_str()
                        && type_name.starts_with("http")
                        && let Some(short) = iri_short_name(type_name)
                    {
                        *t = Value::String(short);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
        }
    }

    let keys_to_rename: Vec<(String, String)> = obj
        .keys()
        .filter(|k| !reserved.contains(k.as_str()))
        .filter_map(|key| {
            if !key.starts_with("http") && !key.starts_with("urn:") {
                return None;
            }
            iri_short_name(key).map(|short| (key.clone(), short)) // clone: keys are borrowed from map iterator
        })
        .filter(|(old_key, new_key)| old_key != new_key && !obj.contains_key(new_key))
        .collect();

    for (old_key, new_key) in keys_to_rename {
        if let Some(value) = obj.remove(&old_key) {
            obj.insert(new_key, value);
        }
    }
}

/// Service layer for NGSI-LD Temporal Entity operations (read and write).
pub struct TemporalService {
    reader: Arc<dyn TemporalHistoryReader>,
    mutator: Arc<dyn TemporalMutator>,
    recorder: Arc<TemporalRecorder>,
    writes: Arc<dyn WriteCoordinator>,
    entity_repo: Arc<dyn EntityStore>,
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
    /// Create a new temporal service.
    ///
    /// The reader and mutator ports decouple the service from the concrete
    /// store implementation. The recorder and write coordinator handle the
    /// bulk-write path, while entity_repo provides base entity lookups.
    pub fn new(
        entity_repo: Arc<dyn EntityStore>,
        reader: Arc<dyn TemporalHistoryReader>,
        mutator: Arc<dyn TemporalMutator>,
        recorder: Arc<TemporalRecorder>,
        writes: Arc<dyn WriteCoordinator>,
        engine: JsonLdEngine,
        default_last_n: u32,
    ) -> TemporalService {
        TemporalService {
            reader,
            mutator,
            recorder,
            writes,
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
    ///
    /// The entity placeholder row and every history row it carries are written
    /// in a single transaction: NGSI-LD 6.18.3.1 defines only 201/204/400 for
    /// `POST /temporal/entities`, so there is no way to report that some
    /// attributes were stored and others were not. A database failure therefore
    /// rolls the request back whole rather than reporting success over partial
    /// history.
    pub async fn create_or_update(&self, mut payload: Value, context: &ContextSpec) -> Result<CreateOrUpdateResult, AndromedaError> {
        normalize_entity_attributes(&mut payload);
        let expanded = self.engine.expand(payload, context).await?;
        let entity_id = {
            let entity_obj = expanded
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_object())
                .ok_or_else(|| AndromedaError::temporal_validation("Entity must be a JSON object"))?;

            let entity_id = entity_obj
                .get("@id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AndromedaError::temporal_validation("Entity missing @id"))?;

            if entity_id.parse::<Uri>().is_err() {
                return Err(AndromedaError::temporal_validation("Invalid entity id"));
            }
            if entity_obj.get("@type").and_then(|v| v.as_array()).is_none_or(|types| types.is_empty()) {
                return Err(AndromedaError::temporal_validation("Entity missing @type"));
            }
            entity_id.to_string()
        };

        // Per the specification, temporal entities can be created independently
        // of the entity table, so the entity is not required to exist first.
        let exists = self.mutator.entity_exists(&entity_id).await?;

        let payload = Arc::new(expanded);

        if exists {
            self.add_attrs_internal(&entity_id, payload).await?;
            Ok(CreateOrUpdateResult::Updated)
        } else {
            // The base entity row is a placeholder carrying the expanded
            // payload, so the stored JSON has the `@type` every entity read
            // expects. No pre-flight existence check: the statement's conflict
            // clause is the check, and it does not lose a race to a concurrent
            // temporal write for the same new identifier.
            let write = AtomicWrite::builder()
                .entity_id(Arc::from(entity_id.as_str()))
                // Unconditional: two concurrent temporal writes for one new
                // identifier are both legitimate, and neither should fail
                // because the other materialised the row first.
                .entity(EntityMutation::Insert(EntityRecord::from_expanded(Arc::clone(&payload)))) // clone: Arc ref-count bump
                .history(vec![HistoryRecord::Observations(payload)])
                .build();
            self.writes.commit(vec![write]).await?;

            Ok(CreateOrUpdateResult::Created(entity_id))
        }
    }

    /// Add temporal attributes to an existing temporal entity.
    pub async fn add_attrs(&self, entity_id: &str, mut payload: Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if !self.mutator.entity_exists(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }

        normalize_entity_attributes(&mut payload);
        let expanded = self.engine.expand(payload, context).await?;
        self.add_attrs_internal(entity_id, Arc::new(expanded)).await?;
        Ok(())
    }

    /// Append every observation in `expanded` to the entity's Temporal Evolution
    /// in one transaction.
    ///
    /// No entity placeholder is written here: the caller has already established
    /// that the entity has `temporal_entity_attributes` rows, and their foreign
    /// key to `entities(id)` cascades on delete, so the base row is necessarily
    /// present.
    ///
    /// A database failure fails the whole request. NGSI-LD 6.20.3.1 defines only
    /// 204/400/404 for `POST /temporal/entities/{id}/attrs`, and 204 means "all
    /// the Attributes were added successfully" -- there is no response code that
    /// can report a subset having been stored.
    async fn add_attrs_internal(&self, entity_id: &str, expanded: Arc<Value>) -> Result<(), AndromedaError> {
        let write = AtomicWrite::builder()
            .entity_id(Arc::from(entity_id))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(expanded)])
            .build();
        self.writes.commit(vec![write]).await?;
        Ok(())
    }

    async fn resolve_attr_iri_for_entity(&self, entity_id: &str, attr_id: &str, context: &ContextSpec) -> Result<String, AndromedaError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expanded_attr = self.engine.expand_term(attr_id, &prepared_context).await?.to_string();

        if !self.mutator.attribute_entries(entity_id, &expanded_attr).await?.is_empty() {
            return Ok(expanded_attr);
        }

        if expanded_attr != attr_id && !self.mutator.attribute_entries(entity_id, attr_id).await?.is_empty() {
            return Ok(attr_id.to_string());
        }

        let attr_names = self.mutator.attribute_names_for_entity(entity_id).await?;
        let mut matches = attr_names
            .into_iter()
            .filter(|name| iri_short_name(name).is_some_and(|short| short == attr_id))
            .collect::<Vec<_>>();

        if matches.len() == 1
            && let Some(name) = matches.pop()
        {
            return Ok(name);
        }

        Ok(expanded_attr)
    }

    /// Delete all temporal data for an entity.
    pub async fn delete(&self, entity_id: &str) -> Result<(), AndromedaError> {
        if !self.mutator.entity_exists(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }
        self.mutator.delete_all_for_entity(entity_id).await?;
        Ok(())
    }

    /// Delete a specific temporal attribute (optionally filtered by dataset ID).
    pub async fn delete_attr(
        &self,
        entity_id: &str,
        attr_id: &str,
        dataset_id: Option<&Urn>,
        scope: DeleteScope,
        context: &ContextSpec,
    ) -> Result<(), AndromedaError> {
        let attr_iri = self.resolve_attr_iri_for_entity(entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }

        let teas = self.mutator.attribute_entries(entity_id, &attr_iri).await?;
        if teas.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute {} not found on entity {}",
                attr_iri, entity_id
            )));
        }

        let deleted = self.mutator.delete_attribute(entity_id, &attr_iri, dataset_id, scope).await?;
        if deleted == 0 {
            let target = match dataset_id {
                Some(ds_id) => format!("Attribute {} with datasetId {} not found on entity {}", attr_iri, ds_id, entity_id),
                None => format!("Attribute {} default instance not found on entity {}", attr_iri, entity_id),
            };
            return Err(AndromedaError::temporal_not_found(target));
        }
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
        let attr_iri = self.resolve_attr_iri_for_entity(entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(format!("Entity {} not found", entity_id)));
        }

        let teas = self.mutator.attribute_entries(entity_id, &attr_iri).await?;
        if teas.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute {} not found on entity {}",
                attr_iri, entity_id
            )));
        }

        if !self.mutator.get_instance(instance_id, entity_id, &attr_iri).await? {
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {} not found", instance_id)));
        }

        let expanded = self.engine.expand(payload, context).await?;
        let instance_value = expanded.as_array().and_then(|arr| arr.first()).cloned().unwrap_or(expanded); // clone: extracting first element from array requires owned value; original consumed by unwrap_or

        self.recorder
            .update_attribute_instance(entity_id, &attr_iri, instance_id, &instance_value)
            .await
            .map_err(AndromedaError::from)
    }

    /// Delete a specific attribute instance by its instance ID.
    pub async fn delete_attr_instance(&self, entity_id: &str, attr_id: &str, instance_id: &str, context: &ContextSpec) -> Result<(), AndromedaError> {
        let attr_iri = self.resolve_attr_iri_for_entity(entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(format!("Entity {} not found", entity_id)));
        }

        let teas = self.mutator.attribute_entries(entity_id, &attr_iri).await?;
        if teas.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute {} not found on entity {}",
                attr_iri, entity_id
            )));
        }

        if !self.mutator.get_instance(instance_id, entity_id, &attr_iri).await? {
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {} not found", instance_id)));
        }

        self.mutator.delete_instance(instance_id, entity_id, &attr_iri).await?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Read operations
    // -------------------------------------------------------------------------

    /// Retrieve the temporal representation of a single entity.
    pub async fn get(&self, entity_id: &str, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalReadResult, AndromedaError> {
        params.validate(false)?;

        if !self.mutator.entity_exists(entity_id).await? {
            return Err(AndromedaError::temporal_not_found(entity_id.to_string()));
        }

        let entity_types = match entity_id.parse::<Uri>() {
            Ok(entity_uri) => self.entity_repo.get(&entity_uri).await?.as_ref().map(extract_types).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let compiled = self.compile_temporal_query(params, context).await?;

        let format = compiled.representation.format;

        if format == TemporalFormat::AggregatedValues {
            let agg_rows = self.reader.query_entity_aggregation(entity_id, &compiled).await?;
            let result = self.build_aggregated_compacted(entity_id, &entity_types, &agg_rows, &compiled, context).await?;
            return Ok(TemporalReadResult {
                value: result,
                content_range: None,
            });
        }

        let outcome = self.reader.query_entity_history(entity_id, &compiled).await?;

        // When an attrs filter is specified and nothing matches, return 404
        if outcome.rows.is_empty() && !compiled.instances.attr_filter.is_empty() {
            return Err(AndromedaError::temporal_not_found(format!(
                "No temporal data found for the specified attributes on entity {}",
                entity_id
            )));
        }

        let result = TemporalEntityRenderer::new(entity_id, &entity_types, &outcome.rows, &compiled).render();
        let compacted = self.engine.compact(result.entity, context, CompactOptions::default()).await?;
        let mut compacted = TemporalArrayNormalizer::new(compacted).render();
        if !has_explicit_context(context) {
            normalize_temporal_payload_without_context(&mut compacted);
        }

        let value = if format == TemporalFormat::TemporalValues {
            TemporalValuesRenderer::new(compacted).render()
        } else {
            compacted
        };

        let content_range = if result.truncated {
            build_content_range(&compiled, result.time_range, result.max_partition_total, compiled.instances.last_n_origin)
        } else {
            None
        };

        Ok(TemporalReadResult { value, content_range })
    }

    /// Query temporal entities matching the given filter and temporal parameters.
    pub async fn query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalQueryResult, AndromedaError> {
        params.validate(true)?;

        let compiled = self.compile_temporal_query(params, context).await?;

        let id_outcome = self.reader.query_entity_ids(&compiled).await?;
        let entity_ids = id_outcome.entity_ids;
        let total_count = id_outcome.total_count;

        // Batch-fetch entity payloads to extract types (avoids N+1 queries).
        // Stage 1 read these ids out of `entities.id`, so any that fails to parse
        // is a corrupt row and is simply absent from the type map below.
        let entity_uris: Vec<Uri> = entity_ids.iter().filter_map(|id| id.parse::<Uri>().ok()).collect();
        let entity_payloads = self.entity_repo.get_batch(&entity_uris).await?;
        let entity_types_map: std::collections::HashMap<String, Vec<EntityTypeName>> = entity_payloads
            .iter()
            .map(|(id, payload)| (id.to_string(), extract_types(payload))) // key by text: the loop below carries String ids from stage 1
            .collect();

        let mut entities = Vec::new();
        let mut warnings = Vec::new();
        let mut any_truncated = false;
        let mut overall_min: Option<DateTime<Utc>> = None;
        let mut overall_max: Option<DateTime<Utc>> = None;
        let mut overall_partition_total: Option<i64> = None;
        let is_aggregated = compiled.representation.format == TemporalFormat::AggregatedValues;
        for entity_id in entity_ids {
            let entity_types = entity_types_map.get(&entity_id).cloned().unwrap_or_default(); // clone: owned copy needed; map is borrowed while loop continues
            if is_aggregated {
                match self.reader.query_entity_aggregation(&entity_id, &compiled).await {
                    Ok(agg_rows) => {
                        if !agg_rows.is_empty() {
                            match self.build_aggregated_compacted(&entity_id, &entity_types, &agg_rows, &compiled, context).await {
                                Ok(result) => entities.push(result),
                                Err(e) => {
                                    tracing::warn!(error = %e, entity_id = %entity_id, "Failed to build aggregated temporal result");
                                    warnings.push(TemporalQueryWarning {
                                        entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                                        message: e.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, entity_id = %entity_id, "Failed to query temporal aggregation");
                        warnings.push(TemporalQueryWarning {
                            entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                            message: e.to_string(),
                        });
                    }
                }
            } else {
                match self.reader.query_entity_history(&entity_id, &compiled).await {
                    Ok(outcome) => {
                        if !outcome.rows.is_empty() {
                            let result = TemporalEntityRenderer::new(&entity_id, &entity_types, &outcome.rows, &compiled).render();
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
                                Ok(compacted) => {
                                    let mut compacted = TemporalArrayNormalizer::new(compacted).render();
                                    if !has_explicit_context(context) {
                                        normalize_temporal_payload_without_context(&mut compacted);
                                    }
                                    let result = if compiled.representation.format == TemporalFormat::TemporalValues {
                                        TemporalValuesRenderer::new(compacted).render()
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
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, entity_id = %entity_id, "Failed to query temporal history");
                        warnings.push(TemporalQueryWarning {
                            entity_id: entity_id.clone(), // clone: entity_id is used by subsequent loop iterations
                            message: e.to_string(),
                        });
                    }
                }
            }
        }

        let content_range = if any_truncated {
            build_content_range(
                &compiled,
                overall_min.zip(overall_max),
                overall_partition_total,
                compiled.instances.last_n_origin,
            )
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
        entity_types: &[EntityTypeName],
        rows: &[TemporalAggregationRow],
        compiled: &CompiledTemporalQuery,
        context: &ContextSpec,
    ) -> Result<Value, AndromedaError> {
        // Build aggregation data keyed by attribute IRI
        let aggr_data = TemporalAggregationRenderer::new(rows, compiled).render();

        // Compact each attribute IRI individually by building a 1-key skeleton and compacting it
        let mut iri_to_short: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for attr_iri in aggr_data.keys() {
            let mut skeleton = serde_json::Map::new();
            skeleton.insert("@id".to_string(), json!(entity_id));
            skeleton.insert(attr_iri.clone(), json!([{"@type": [expanded_classes::PROPERTY]}])); // clone: owned key for JSON map insertion
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
            if !iri_to_short.contains_key(attr_iri) {
                let short = compact_attr_iri(&self.engine, attr_iri, context).await;
                if short != *attr_iri {
                    iri_to_short.insert(attr_iri.clone(), short);
                } else if !has_explicit_context(context)
                    && let Some(short_name) = iri_short_name(attr_iri)
                    && short_name != *attr_iri
                {
                    iri_to_short.insert(attr_iri.clone(), short_name);
                }
            }
        }

        // Compact entity type by compacting a minimal type skeleton
        let type_skeleton = json!({"@id": entity_id, "@type": entity_types});
        let compacted_type = if let Ok(compacted) = self.engine.compact(type_skeleton, context, CompactOptions::default()).await {
            let mut compacted_type = compacted.as_object().and_then(|o| o.get("type")).cloned().unwrap_or(json!(entity_types)); // clone: need owned Value from borrowed compacted object
            if !has_explicit_context(context) {
                match &mut compacted_type {
                    Value::String(type_name) => {
                        if type_name.starts_with("http")
                            && let Some(short) = iri_short_name(type_name)
                        {
                            *type_name = short;
                        }
                    }
                    Value::Array(types) => {
                        for type_value in types {
                            if let Some(type_name) = type_value.as_str()
                                && type_name.starts_with("http")
                                && let Some(short) = iri_short_name(type_name)
                            {
                                *type_value = Value::String(short);
                            }
                        }
                    }
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
                }
            }
            compacted_type
        } else {
            let type_values: Vec<String> = if has_explicit_context(context) {
                entity_types.iter().map(ToString::to_string).collect()
            } else {
                entity_types
                    .iter()
                    .map(|type_name| iri_short_name(type_name.as_str()).unwrap_or_else(|| type_name.to_string()))
                    .collect()
            };
            json!(type_values)
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
        let outcome = self.reader.query_entity_ids(&compiled).await?;
        Ok(outcome.entity_ids)
    }

    /// Merge local temporal query results with forwarded context source results.
    ///
    /// Returns the merged entity values and any warnings generated during forwarding.
    /// When the forwarded list is empty, returns the local entities unchanged.
    pub fn merge_with_forwarded(local_entities: Vec<Value>, forwarded: Vec<ForwardedTemporalEntities>) -> Vec<Value> {
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
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        // Clause 5.2.11: a POST temporal query body's `entities[]` is a union of
        // selectors, compiled by the same helper the entity path uses so the two
        // cannot disagree on what a selection means.
        let selectors = compile_entity_selectors(
            &self.engine,
            &prepared_context,
            &params.entity_selectors,
            params.id.clone().unwrap_or_default(),
            params.id_pattern.as_ref(),
            params.entity_type.as_ref(),
        )
        .await?;

        let attr_filter = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;

        let dataset_id_filter = params
            .dataset_id
            .as_deref()
            .map(|dataset_ids| dataset_ids.iter().cloned().map(DatasetSelector::Id).collect())
            .unwrap_or_default();

        let timerel = params.timerel;

        let aggr_period_duration = params.aggr_period_duration;

        // Expansion consumes the AST while the caller retains its request parameters.
        let q_expr = match params.q.clone() {
            Some(expr) => Some(expand_q_expr(expr, &expander).await?),
            None => None,
        };

        // Apply expandValues: expand string RHS values for specified attributes (spec temporal params)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref ev) = params.expand_values {
                let ev_iris = expand_attrs(ev, &expander).await?;
                let ev_set = opted_in_attribute_names(&ev_iris)?;
                Some(apply_expand_values(q_expr, &ev_set, &expander).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        // Apply jsonKeys: expand sub-path keys for specified attributes (spec 6.4.3.2)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref jk) = params.json_keys {
                let jk_iris = expand_attrs(jk, &expander).await?;
                let jk_set = opted_in_attribute_names(&jk_iris)?;
                Some(apply_json_keys(q_expr, &jk_set, &expander).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        let geo = self.compile_geo_query(params, &prepared_context).await?;

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

        Ok(CompiledTemporalQuery::builder()
            .entities(
                EntitySelection::builder()
                    .selectors(selectors)
                    .q_expr(q_expr)
                    .geo(geo)
                    .scope_q(params.scope_q.clone())
                    .build(),
            )
            .window(
                TemporalWindow::builder()
                    .interval(TimeInterval::from_parts(timerel, params.time_at, params.end_time_at)?)
                    .timeproperty(params.timeproperty_or_default())
                    .build(),
            )
            .instances(
                InstanceSelection::builder()
                    .attr_filter(attr_filter)
                    .dataset_id_filter(dataset_id_filter)
                    .last_n(effective_last_n)
                    .last_n_origin(last_n_origin)
                    .build(),
            )
            .representation(
                Representation::builder()
                    .format(format)
                    .aggr_methods(params.aggr_methods.clone().unwrap_or_default())
                    .aggr_period_duration(aggr_period_duration)
                    .sys_attrs(if params.has_sys_attrs() {
                        SysAttrsInclusion::Include
                    } else {
                        SysAttrsInclusion::Strip
                    })
                    .build(),
            )
            .pagination(Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                count: match params.count {
                    Some(true) => CountPolicy::Emit,
                    Some(false) | None => CountPolicy::Omit,
                },
            })
            .build())
    }

    async fn compile_geo_query(&self, params: &TemporalQueryParameters, context: &PreparedTermContext) -> Result<Option<GeoQuery>, AndromedaError> {
        geo_compile::compile_geo_query(params.geo.0.as_ref(), &ContextualExpander::new(&self.engine, context))
            .await
            .map_err(AndromedaError::from)
    }
}

/// Read an expanded `expandValues` / `jsonKeys` selection as the attribute
/// names a `q` filter is keyed by (clause 6.4.3.2).
fn opted_in_attribute_names(iris: &[IriBuf]) -> Result<HashSet<AttributeName>, QueryError> {
    iris.iter().map(expanded_attribute_name).collect()
}
