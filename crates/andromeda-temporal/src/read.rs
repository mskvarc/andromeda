//! Temporal retrieval and query execution policy.

use crate::{
    error::TemporalError,
    service::{
        TemporalQueryResult,
        TemporalQueryWarning,
        TemporalRange,
        TemporalReadResult,
        TemporalService,
        has_explicit_context,
        iri_short_name,
        normalize_temporal_payload_without_context,
    },
};
use andromeda_jsonld::{compaction::compact_attr_iri, context_spec::ContextSpec, engine::options::CompactOptions};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_model::{expanded::entity_member::extract_types, temporal::TemporalAggregationRow};
use andromeda_ngsi_ld_query::{pagination::Pagination, temporal::compiled::CompiledTemporalQuery};
use andromeda_ngsi_ld_render::temporal::{
    aggregation::TemporalAggregationRenderer,
    array_normalizer::TemporalArrayNormalizer,
    entity::TemporalEntityRenderer,
    values::TemporalValuesRenderer,
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;

impl TemporalService {
    /// Retrieve and render one temporal entity under the supplied query parameters.
    pub async fn get(&self, entity_id: &Uri, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalReadResult, TemporalError> {
        params.validate(false)?;

        let compiled = self.compile_temporal_query(params, context).await?;
        ensure_aggregation_supported(self.reader.as_ref(), &compiled.representation.aggr_methods)?;

        if !self.mutator.entity_exists(entity_id).await? {
            return Err(TemporalError::not_found(entity_id.to_string()));
        }

        let entity_types = self
            .entity_reader
            .get(entity_id)
            .await
            .map_err(|error| TemporalError::persistence(error.to_string()))?
            .as_ref()
            .map(extract_types)
            .unwrap_or_default();

        let format = compiled.representation.format;

        if format == TemporalFormat::AggregatedValues {
            let agg_rows = self.reader.query_entity_aggregation(entity_id, &compiled).await?;
            let result = self.build_aggregated_compacted(entity_id, &entity_types, &agg_rows, &compiled, context).await?;
            return Ok(TemporalReadResult { value: result, range: None });
        }

        let outcome = self.reader.query_entity_history(entity_id, &compiled).await?;

        // When an attrs filter is specified and nothing matches, return 404
        if outcome.rows.is_empty() && !compiled.instances.attr_filter.is_empty() {
            return Err(TemporalError::not_found(format!(
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
            TemporalValuesRenderer::new(compacted)
                .render()
                .map_err(|error| TemporalError::rendering(error.to_string()))?
        } else {
            compacted
        };

        let range = if result.truncated {
            Some(TemporalRange {
                interval: compiled.window.interval,
                observed: result.time_range,
                partition_total: result.max_partition_total,
                last_n_origin: compiled.instances.last_n_origin,
            })
        } else {
            None
        };

        Ok(TemporalReadResult { value, range })
    }

    /// Query temporal entities matching the given filter and temporal parameters.
    pub async fn query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalQueryResult, TemporalError> {
        params.validate(true)?;

        let compiled = self.compile_temporal_query(params, context).await?;

        ensure_aggregation_supported(self.reader.as_ref(), &compiled.representation.aggr_methods)?;
        let id_outcome = self.reader.query_entity_ids(&compiled).await?;
        let (entity_ids, total_count) = id_outcome
            .complete(compiled.pagination)
            .map_err(|error| TemporalError::internal(error.to_string()))?;

        // Batch-fetch entity payloads to extract types (avoids N+1 queries).
        let entity_payloads = self
            .entity_reader
            .get_batch(&entity_ids)
            .await
            .map_err(|error| TemporalError::persistence(error.to_string()))?;
        let entity_types_map: HashMap<Uri, Vec<EntityTypeName>> = entity_payloads
            .iter()
            .map(|(id, payload)| (id.clone(), extract_types(payload))) // clone: the type index owns its validated entity URI.
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
                                    if compiled.representation.format == TemporalFormat::TemporalValues {
                                        match TemporalValuesRenderer::new(compacted).render() {
                                            Ok(result) => entities.push(result),
                                            Err(error) => {
                                                tracing::warn!(error = %error, entity_id = %entity_id, "Failed to render temporal values");
                                                warnings.push(TemporalQueryWarning {
                                                    entity_id: entity_id.clone(), // clone: the warning owns the identifier while the loop continues.
                                                    message: error.to_string(),
                                                });
                                            }
                                        }
                                    } else {
                                        entities.push(compacted);
                                    }
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

        let range = if any_truncated {
            Some(TemporalRange {
                interval: compiled.window.interval,
                observed: overall_min.zip(overall_max),
                partition_total: overall_partition_total,
                last_n_origin: compiled.instances.last_n_origin,
            })
        } else {
            None
        };

        Ok(TemporalQueryResult {
            entities,
            total_count,
            any_truncated,
            range,
            warnings,
        })
    }

    /// Build an aggregated temporal response in compacted form, bypassing JSON-LD compaction.
    /// Aggregation keys (avg, min, etc.) aren't valid JSON-LD IRIs and would be dropped by compact().
    async fn build_aggregated_compacted(
        &self,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        rows: &[TemporalAggregationRow],
        compiled: &CompiledTemporalQuery,
        context: &ContextSpec,
    ) -> Result<Value, TemporalError> {
        // Build aggregation data keyed by attribute IRI
        let aggr_data = TemporalAggregationRenderer::new(rows, compiled).render();

        // Compact each attribute IRI individually by building a 1-key skeleton and compacting it
        let mut iri_to_short: HashMap<AttributeName, AttributeName> = HashMap::new();
        for attr_iri in aggr_data.keys() {
            let mut skeleton = serde_json::Map::new();
            skeleton.insert("@id".to_string(), json!(entity_id));
            skeleton.insert(attr_iri.to_string(), json!([{"@type": [expanded_classes::PROPERTY]}]));
            if let Ok(compacted) = self.engine.compact(Value::Object(skeleton), context, CompactOptions::default()).await
                && let Some(obj) = compacted.as_object()
            {
                for key in obj.keys() {
                    if !["id", "type", "@context"].contains(&key.as_str()) {
                        if let Ok(short) = key.parse() {
                            iri_to_short.insert(attr_iri.clone(), short); // clone: the index owns its attribute key.
                        }
                        break;
                    }
                }
            }
            if !iri_to_short.contains_key(attr_iri) {
                let short = compact_attr_iri(&self.engine, attr_iri.as_str(), context).await;
                if short != attr_iri.as_str()
                    && let Ok(short) = short.parse()
                {
                    iri_to_short.insert(attr_iri.clone(), short); // clone: the index owns its attribute key.
                } else if !has_explicit_context(context)
                    && let Some(short_name) = iri_short_name(attr_iri.as_str())
                    && short_name != attr_iri.as_str()
                    && let Ok(short_name) = short_name.parse()
                {
                    iri_to_short.insert(attr_iri.clone(), short_name); // clone: the index owns its attribute key.
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
            let short_name = iri_to_short.get(&attr_iri).unwrap_or(&attr_iri);
            result.insert(short_name.to_string(), attr_obj);
        }

        Ok(Value::Object(result))
    }

    /// Execute stage 1 entity ID resolution without fetching temporal history.
    ///
    /// Used to build EntityMaps that represent the full result set (spec 5.5.14).
    /// Runs only the lightweight stage 1 SQL (entity ID resolution) without fetching
    /// temporal history, ensuring the EntityMap captures ALL matching IDs, not just the
    /// current page.
    pub async fn query_ids(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<Vec<Uri>, TemporalError> {
        params.validate(true)?;
        let compiled = self.compile_temporal_query(params, context).await?;
        let mut unpaged = compiled.clone();
        unpaged.pagination = Pagination {
            limit: u32::MAX,
            offset: 0,
            count: andromeda_ngsi_ld_query::pagination::CountPolicy::Omit,
        };
        ensure_aggregation_supported(self.reader.as_ref(), &unpaged.representation.aggr_methods)?;
        let outcome = self.reader.query_entity_ids(&unpaged).await?;
        let (identifiers, _total) = outcome
            .complete(unpaged.pagination)
            .map_err(|error| TemporalError::internal(error.to_string()))?;
        Ok(identifiers)
    }
}

/// Reject unsupported aggregation before invoking any reader query operation.
fn ensure_aggregation_supported(
    reader: &dyn crate::port::history_reader::TemporalHistoryReader,
    requested: &[andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod],
) -> Result<(), TemporalError> {
    let refused = reader.refused_aggregation_methods(requested);
    if refused.is_empty() {
        Ok(())
    } else {
        Err(TemporalError::operation_not_supported(format!(
            "temporal reader refused aggregation methods: {refused:?}"
        )))
    }
}

#[cfg(test)]
mod tests;
