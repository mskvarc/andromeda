//! Snapshot query executor (ETSI GS CIM 009 v1.9.1 clause 5.16.1.4).
//!
//! Orchestrates [`EntityQueryService`], [`TemporalService`], and
//! [`CsourceForwardingService`] to populate a snapshot's frozen data
//! using the full NGSI-LD query pipeline.
//!
//! A [`SnapshotQuery`] (clause 5.2.23) is translated into an
//! [`EntityQueryParameters`] or [`TemporalQueryParameters`], the query is
//! executed locally, optionally merged with CSR-forwarded results
//! (clause 5.13), and the resulting entities are persisted into
//! `snapshot_entities` / `snapshot_temporal_entities`.

use crate::service::{
    csource_forwarding::{CsourceForwardingService, CsrModeFilter, ForwardQueryRequest, ForwardingOperation},
    entity::query::EntityQueryService,
    snapshot::store::{SnapshotEntityInput, SnapshotStore},
    snapshot_status::validate_preparing_transition,
    temporal::TemporalService,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::SnapshotId, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, ExecutionResultStatus, Snapshot, SnapshotStatus},
    snapshot_query::{SnapshotEntitySelector, SnapshotGeoQuery, SnapshotQuery, SnapshotTemporalQuery},
};
use andromeda_ngsi_ld_query::{geo::input::GeoQueryInput, pattern::Pattern};
use andromeda_ngsi_ld_request::query::{
    deserialization::OptionalGeoQuery,
    entity::parameters::EntityQueryParameters,
    entity_selector::EntitySelectorParams,
    temporal::TemporalQueryParameters,
};
use compact_str::CompactString;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// Translates NGSI-LD snapshot queries into the service-level query pipeline
/// and persists results.
pub struct SnapshotExecutor {
    snapshot_repo: Arc<dyn SnapshotStore>,
    entity_query: Arc<EntityQueryService>,
    temporal: Arc<TemporalService>,
    forwarding: Arc<CsourceForwardingService>,
}

/// Failure modes when translating a [`SnapshotQuery`] into the pipeline-level
/// parameter types.
#[derive(Debug, Error)]
pub enum SnapshotQueryTranslationError {
    /// A snapshot temporal query did not include the required `temporalQ` object.
    #[error("snapshot temporal query missing required 'temporalQ' section (clause 5.2.23)")]
    MissingTemporalQ,
    /// Multiple selectors disagreed on `idPattern`; only one is representable
    /// in [`EntityQueryParameters`].
    #[error("multiple snapshot entity selectors specify conflicting idPatterns")]
    ConflictingIdPattern(CompactString, CompactString),
    /// A persisted snapshot contained an invalid geo-query geometry.
    #[error("snapshot geo-query geometry is invalid: {0}")]
    Geo(#[from] andromeda_ngsi_ld_geometry::geometry::GeometryError),
}

impl SnapshotExecutor {
    /// Create a new snapshot executor wired to the query services.
    pub fn new(
        snapshot_repo: Arc<dyn SnapshotStore>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<CsourceForwardingService>,
    ) -> SnapshotExecutor {
        SnapshotExecutor {
            snapshot_repo,
            entity_query,
            temporal,
            forwarding,
        }
    }

    /// Execute all queries for a snapshot and update its status.
    pub async fn execute(&self, snapshot: &Snapshot) {
        info!(snapshot_id = %snapshot.id, "Starting snapshot query execution");

        let mut queries_details = Vec::new();
        let mut temporal_queries_details = Vec::new();
        let mut any_success_with_results = false;
        let mut any_failure = false;
        let mut all_empty = true;

        if let Some(queries) = &snapshot.snapshot_queries {
            for (idx, query) in queries.iter().enumerate() {
                let detail = self.run_entity_query(&snapshot.id, query, idx).await;
                update_aggregate_flags(&detail, &mut any_success_with_results, &mut any_failure, &mut all_empty);
                queries_details.push(detail);
            }
        }

        if let Some(queries) = &snapshot.snapshot_temporal_queries {
            for (idx, query) in queries.iter().enumerate() {
                let detail = self.run_temporal_query(&snapshot.id, query, idx).await;
                update_aggregate_flags(&detail, &mut any_success_with_results, &mut any_failure, &mut all_empty);
                temporal_queries_details.push(detail);
            }
        }

        let final_status = compute_final_status(any_success_with_results, any_failure, all_empty);
        debug!(snapshot_id = %snapshot.id, status = %final_status, "Snapshot query execution completed");

        let qd = if queries_details.is_empty() { None } else { Some(queries_details.as_slice()) };
        let tqd = if temporal_queries_details.is_empty() {
            None
        } else {
            Some(temporal_queries_details.as_slice())
        };

        if let Err(e) = validate_preparing_transition(final_status) {
            error!(snapshot_id = %snapshot.id, error = %e, "Invalid status transition requested");
            return;
        }
        if let Err(e) = self.snapshot_repo.update_status(&snapshot.id, final_status, qd, tqd).await {
            error!(
                snapshot_id = %snapshot.id,
                error = %e,
                "Failed to update snapshot status after execution"
            );
        }
    }

    async fn run_entity_query(&self, snapshot_id: &SnapshotId, query: &SnapshotQuery, idx: usize) -> ExecutionResultDetails {
        let params = match snapshot_query_to_entity_params(&query) {
            Ok(params) => params,
            Err(error) => return failure_detail(error.to_string()),
        };

        if let Err(error) = params.validate() {
            return failure_detail(format!("Snapshot query translation failed validation: {error}"));
        }

        let context = ContextSpec::core_only();
        let (mut entities, _count) = match self.entity_query.query(&params, &context).await {
            Ok(pair) => pair,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = idx, error = %error, "Snapshot entity query failed");
                return failure_detail(error.to_string());
            }
        };

        // Aggregate CSR-forwarded results (clause 5.13 + 5.16.1). Failures here
        // do not fail the snapshot query; they are logged and ignored.
        let entity_ids = params.id.clone();
        let entity_type = params.entity_type.as_ref().map(ToString::to_string);
        let forwarded = self
            .forwarding
            .forward_query(ForwardQueryRequest {
                entity_type_compact: entity_type.as_deref(),
                entity_ids: entity_ids.as_deref(),
                context_url: None,
                context: &context,
                operation: ForwardingOperation::QueryBatch,
                incoming_via: None,
                csf_expr: None,
                mode_filter: CsrModeFilter::All,
            })
            .await
            .unwrap_or_else(|error| {
                warn!(snapshot_id = %snapshot_id, query_index = idx, error = %error, "Snapshot CSR forwarding failed");
                Vec::new()
            });

        if !forwarded.is_empty() {
            entities = EntityQueryService::merge_with_forwarded(entities, forwarded, None);
        }

        let pairs: Vec<(String, Value)> = entities
            .into_iter()
            .filter_map(|entity| {
                let id = entity.id.to_string();
                match serde_json::to_value(&entity) {
                    Ok(value) => Some((id, value)),
                    Err(error) => {
                        warn!(snapshot_id = %snapshot_id, query_index = idx, error = %error, entity_id = %id, "Failed to serialize snapshot entity");
                        None
                    }
                }
            })
            .collect();

        let count = pairs.len();
        let inputs: Vec<SnapshotEntityInput<'_>> = pairs.iter().map(|(id, data)| SnapshotEntityInput { entity_id: id, data }).collect();
        if !inputs.is_empty()
            && let Err(error) = self.snapshot_repo.insert_entities(snapshot_id, &inputs).await
        {
            error!(snapshot_id = %snapshot_id, query_index = idx, error = %error, "Failed to store snapshot entity query results");
            return failure_detail(format!("Failed to store results: {error}"));
        }

        success_detail(count)
    }

    async fn run_temporal_query(&self, snapshot_id: &SnapshotId, query: &SnapshotQuery, idx: usize) -> ExecutionResultDetails {
        let params = match snapshot_query_to_temporal_params(&query) {
            Ok(params) => params,
            Err(error) => return failure_detail(error.to_string()),
        };

        let context = ContextSpec::core_only();
        let result = match self.temporal.query(&params, &context).await {
            Ok(result) => result,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = idx, error = %error, "Snapshot temporal query failed");
                return failure_detail(error.to_string());
            }
        };

        let pairs: Vec<(String, Value)> = result
            .entities
            .into_iter()
            .filter_map(|value| {
                let id = value.get("id").and_then(Value::as_str).map(str::to_string)?;
                Some((id, value))
            })
            .collect();

        let count = pairs.len();
        let inputs: Vec<SnapshotEntityInput<'_>> = pairs.iter().map(|(id, data)| SnapshotEntityInput { entity_id: id, data }).collect();
        if !inputs.is_empty()
            && let Err(error) = self.snapshot_repo.insert_temporal_entities(snapshot_id, &inputs).await
        {
            error!(snapshot_id = %snapshot_id, query_index = idx, error = %error, "Failed to store snapshot temporal query results");
            return failure_detail(format!("Failed to store results: {error}"));
        }

        success_detail(count)
    }
}

const fn update_aggregate_flags(detail: &ExecutionResultDetails, any_success_with_results: &mut bool, any_failure: &mut bool, all_empty: &mut bool) {
    match detail.result_status {
        ExecutionResultStatus::Success => {
            *any_success_with_results = true;
            *all_empty = false;
        }
        ExecutionResultStatus::Failure => *any_failure = true,
        ExecutionResultStatus::Empty => {}
    }
}

const fn success_detail(count: usize) -> ExecutionResultDetails {
    ExecutionResultDetails {
        result_status: if count > 0 {
            ExecutionResultStatus::Success
        } else {
            ExecutionResultStatus::Empty
        },
        problem_details: None,
    }
}

fn failure_detail(message: impl Into<String>) -> ExecutionResultDetails {
    ExecutionResultDetails {
        result_status: ExecutionResultStatus::Failure,
        problem_details: Some(serde_json::json!({ "detail": message.into() })),
    }
}

/// Translate a non-temporal [`SnapshotQuery`] into service-layer
/// [`EntityQueryParameters`].
///
/// Extract identifiers and identifier patterns while retaining type expressions
/// in their individual selectors.
pub fn snapshot_query_to_entity_params(query: &SnapshotQuery) -> Result<EntityQueryParameters, SnapshotQueryTranslationError> {
    let (ids, id_pattern) = flatten_selectors(query.entities.as_deref())?;

    let geo = snapshot_geo_query(query.geo_q.as_ref())?;

    Ok(EntityQueryParameters {
        id: if ids.is_empty() { None } else { Some(ids) },
        entity_type: None,
        id_pattern: id_pattern.clone(),
        entity_selectors: snapshot_entity_selectors(query.entities.as_deref()),
        attrs: query.attrs.clone(),
        // The persisted snapshot retains its AST while translation owns a query copy.
        q: query.q.clone(),
        geo,
        limit: None,
        offset: None,
        count: None,
        options: None,
        local: None,
        pick: None,
        omit: query.omit.clone(),
        order_by: None,
        scope_q: query.scope_q.clone(), // clone: the snapshot remains available to a subsequent execution
        lang: None,
        dataset_id: None,
        expand_values: None,
        json_keys: None,
        format: None,
        join: None,
        join_level: None,
        contained_by: None,
        entity_map: None,
        entity_map_lifetime: None,
        split_entities: None,
        geometry_property: None,
        collation: None,
        order_from: None,
        order_geometry: None,
        csf: None,
        drop: None,
        keep: None,
    })
}

/// Translate a temporal [`SnapshotQuery`] into service-layer
/// [`TemporalQueryParameters`].
pub fn snapshot_query_to_temporal_params(query: &SnapshotQuery) -> Result<TemporalQueryParameters, SnapshotQueryTranslationError> {
    let temporal_q: &SnapshotTemporalQuery = query.temporal_q.as_ref().ok_or(SnapshotQueryTranslationError::MissingTemporalQ)?;
    let (ids, id_pattern) = flatten_selectors(query.entities.as_deref())?;

    let geo = snapshot_geo_query(query.geo_q.as_ref())?;

    Ok(TemporalQueryParameters {
        id: if ids.is_empty() { None } else { Some(ids) },
        entity_type: None,
        id_pattern: id_pattern.clone(),
        entity_selectors: snapshot_entity_selectors(query.entities.as_deref()),
        attrs: query.attrs.clone(),
        // The persisted snapshot retains its AST while translation owns a query copy.
        q: query.q.clone(),
        geo,
        timerel: Some(temporal_q.timerel),
        time_at: Some(temporal_q.time_at),
        end_time_at: temporal_q.end_time_at,
        timeproperty: temporal_q.timeproperty,
        last_n: temporal_q.last_n,
        format: None,
        options: None,
        // The translated request owns its parsed methods while the snapshot remains immutable.
        aggr_methods: temporal_q.aggr_methods.clone(),
        aggr_period_duration: temporal_q.aggr_period_duration,
        limit: None,
        offset: None,
        count: None,
        _order_by: None,
        pick: None,
        omit: query.omit.clone(),
        dataset_id: None,
        _lang: None,
        local: None,
        entity_map: None,
        entity_map_lifetime: None,
        split_entities: None,
        scope_q: query.scope_q.clone(), // clone: the snapshot remains available to a subsequent execution
        expand_values: None,
        json_keys: None,
    })
}

/// Convert the snapshot's typed geo-query resource into the request pipeline value.
fn snapshot_geo_query(geo: Option<&SnapshotGeoQuery>) -> Result<OptionalGeoQuery, SnapshotQueryTranslationError> {
    let Some(geo) = geo else {
        return Ok(OptionalGeoQuery::default());
    };
    let value = serde_json::json!({ "type": geo.geometry.to_string(), "coordinates": geo.coordinates });
    let geometry = Geometry::from_geojson_value(&value)?;
    Ok(OptionalGeoQuery(Some(GeoQueryInput::new(
        geo.georel.clone(),
        geometry,
        geo.geoproperty.clone(),
    ))))
}

/// Tuple of `(ids, id_pattern)` extracted from a set of snapshot
/// entity selectors. A type alias keeps the `flatten_selectors` return type
/// readable.
/// The per-selector grouping (clause 5.2.11) a snapshot's `entities[]` carries,
/// preserved alongside the flattened summary [`flatten_selectors`] produces so
/// a union of selectors is not compiled into a cross product.
fn snapshot_entity_selectors(selectors: Option<&[SnapshotEntitySelector]>) -> Vec<EntitySelectorParams> {
    selectors
        .unwrap_or_default()
        .iter()
        .map(|selector| EntitySelectorParams {
            id: selector.id.clone(),
            id_pattern: selector.id_pattern.clone(),
            type_: selector.type_.clone(),
        })
        .collect()
}

type FlattenedSelectors = (Vec<Uri>, Option<Pattern>);

fn flatten_selectors(selectors: Option<&[SnapshotEntitySelector]>) -> Result<FlattenedSelectors, SnapshotQueryTranslationError> {
    let mut ids: Vec<Uri> = Vec::new();
    let mut id_pattern: Option<Pattern> = None;

    let Some(selectors) = selectors else {
        return Ok((ids, id_pattern));
    };

    for selector in selectors {
        if let Some(entity_ids) = &selector.id {
            ids.extend(entity_ids.iter().cloned());
        }
        if let Some(pattern) = &selector.id_pattern {
            match &id_pattern {
                // clone: the flattened result owns the pattern and the
                // selector list it came from is borrowed
                None => id_pattern = Some(pattern.clone()),
                Some(existing) if existing == pattern => {}
                Some(existing) => {
                    return Err(SnapshotQueryTranslationError::ConflictingIdPattern(
                        CompactString::from(existing.to_string()),
                        CompactString::from(pattern.to_string()),
                    ));
                }
            }
        }
    }

    Ok((ids, id_pattern))
}

/// Compute the final snapshot status from per-query execution results.
///
/// - All queries succeeded with results -> `Success`
/// - At least one succeeded with results, others failed/empty -> `Partial`
/// - All succeeded but no results -> `Empty`
/// - All failed -> `Failure`
const fn compute_final_status(any_success_with_results: bool, any_failure: bool, all_empty: bool) -> SnapshotStatus {
    if any_success_with_results && !any_failure {
        SnapshotStatus::Success
    } else if any_success_with_results && any_failure {
        SnapshotStatus::Partial
    } else if all_empty && !any_failure {
        SnapshotStatus::Empty
    } else {
        SnapshotStatus::Failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::geometry::GeometryType;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, time_prop::TimeProp, time_rel::TimeRel};
    use andromeda_ngsi_ld_model::snapshot_query::{SnapshotEntitySelector, SnapshotGeoQuery, SnapshotQuery, SnapshotTemporalQuery};
    use andromeda_ngsi_ld_query::{scope::query::ScopeQuery, temporal::aggregation_period::AggregationPeriod};
    use chrono::{DateTime, Utc};

    fn attribute_name(raw: &str) -> AttributeName {
        raw.parse().expect("valid attribute name")
    }

    fn base_query() -> SnapshotQuery {
        SnapshotQuery::builder().build()
    }

    #[test]
    fn status_all_success() {
        pretty_assertions::assert_eq!(compute_final_status(true, false, false), SnapshotStatus::Success);
    }

    #[test]
    fn status_all_empty() {
        pretty_assertions::assert_eq!(compute_final_status(false, false, true), SnapshotStatus::Empty);
    }

    #[test]
    fn status_mixed_partial() {
        pretty_assertions::assert_eq!(compute_final_status(true, true, false), SnapshotStatus::Partial);
    }

    #[test]
    fn status_all_failure() {
        pretty_assertions::assert_eq!(compute_final_status(false, true, false), SnapshotStatus::Failure);
    }

    #[test]
    fn status_failure_with_all_empty() {
        pretty_assertions::assert_eq!(compute_final_status(false, true, true), SnapshotStatus::Failure);
    }

    #[test]
    fn translates_q_to_entity_params() {
        // Clause 5.7.2, 5.16.1.4
        let mut q = base_query();
        q.q = Some("speed>50".parse().expect("valid q filter"));
        let params = snapshot_query_to_entity_params(&q).expect("translate");
        pretty_assertions::assert_eq!(params.q.as_ref().map(ToString::to_string).as_deref(), Some("speed>50"));
    }

    #[test]
    fn translates_geo_q_to_entity_params() {
        // Clause 5.7.2 geo-query, 5.2.13
        let mut q = base_query();
        q.geo_q = Some(SnapshotGeoQuery {
            georel: "near;maxDistance==1000".parse().expect("valid georel"),
            geometry: GeometryType::Point,
            coordinates: serde_json::json!([13.4, 52.5]),
            geoproperty: Some("location".parse().expect("valid reference")),
        });
        let params = snapshot_query_to_entity_params(&q).expect("translate");
        let geo = params.geo.0.expect("geo query");
        pretty_assertions::assert_eq!(geo.geoproperty.as_ref().map(AsRef::<str>::as_ref), Some("location"));
    }

    #[test]
    fn translates_scope_q_to_entity_params() {
        // Clause 5.5.14 (scopeQ)
        let mut q = base_query();
        q.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        let params = snapshot_query_to_entity_params(&q).expect("translate");
        pretty_assertions::assert_eq!(params.scope_q.as_ref().map(ToString::to_string).as_deref(), Some("/Madrid"));
    }

    #[test]
    fn translates_attrs_and_omit_to_entity_params() {
        // Clause 5.5.9 attrs/omit projection
        let mut q = base_query();
        q.attrs = Some(vec![attribute_name("speed"), attribute_name("fuel")]);
        q.omit = Some(vec![attribute_name("name")]);
        let params = snapshot_query_to_entity_params(&q).expect("translate");
        pretty_assertions::assert_eq!(
            params.attrs.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "fuel"])
        );
        pretty_assertions::assert_eq!(
            params.omit.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["name"])
        );
    }

    #[test]
    fn multi_selector_flattens_ids_and_types() {
        // Clause 5.2.23: EntityInfo selectors with multiple entries
        let mut q = base_query();
        q.entities = Some(vec![
            SnapshotEntitySelector {
                id: Some(vec!["urn:ngsi-ld:Vehicle:1".parse().expect("valid uri")]),
                id_pattern: None,
                type_: Some("Vehicle".parse().expect("valid type expression")),
            },
            SnapshotEntitySelector {
                id: Some(vec!["urn:ngsi-ld:Vehicle:2".parse().expect("valid uri")]),
                id_pattern: None,
                type_: Some("Truck".parse().expect("valid type expression")),
            },
        ]);
        let params = snapshot_query_to_entity_params(&q).expect("translate");
        let ids = params.id.as_deref().unwrap();
        assert!(ids.iter().any(|id| id.as_str() == "urn:ngsi-ld:Vehicle:1"));
        assert!(ids.iter().any(|id| id.as_str() == "urn:ngsi-ld:Vehicle:2"));
        assert!(params.entity_type.is_none());
        pretty_assertions::assert_eq!(params.entity_selectors.len(), 2);
    }

    #[test]
    fn id_pattern_propagates_from_selector() {
        let mut q = base_query();
        q.entities = Some(vec![SnapshotEntitySelector {
            id: None,
            id_pattern: Some(Pattern::parse("urn:ngsi-ld:Vehicle:.*").expect("valid pattern")),
            type_: None,
        }]);
        let params = snapshot_query_to_entity_params(&q).expect("translate");
        pretty_assertions::assert_eq!(params.id_pattern.map(|pattern| pattern.to_string()), Some("urn:ngsi-ld:Vehicle:.*".to_string()));
    }

    #[test]
    fn conflicting_id_patterns_are_rejected() {
        let mut q = base_query();
        q.entities = Some(vec![
            SnapshotEntitySelector {
                id: None,
                id_pattern: Some(Pattern::parse("urn:ngsi-ld:A:.*").expect("valid pattern")),
                type_: None,
            },
            SnapshotEntitySelector {
                id: None,
                id_pattern: Some(Pattern::parse("urn:ngsi-ld:B:.*").expect("valid pattern")),
                type_: None,
            },
        ]);
        let err = snapshot_query_to_entity_params(&q).expect_err("conflict");
        assert!(matches!(err, SnapshotQueryTranslationError::ConflictingIdPattern(..)));
    }

    /// Parse an RFC 3339 literal into the instant a `temporalQ` carries.
    fn instant(literal: &str) -> DateTime<Utc> {
        literal.parse::<DateTime<Utc>>().expect("valid instant")
    }

    #[test]
    fn temporal_translates_timerel_time_at() {
        // Clause 5.7.3, 5.2.23 temporalQ
        let mut q = base_query();
        q.temporal_q = Some(
            SnapshotTemporalQuery::builder()
                .timerel(TimeRel::After)
                .time_at(instant("2024-01-01T00:00:00Z"))
                .end_time_at(None)
                .last_n(None)
                .timeproperty(Some(TimeProp::Observed))
                .aggr_methods(None)
                .aggr_period_duration(None)
                .build(),
        );
        let params = snapshot_query_to_temporal_params(&q).expect("translate");
        pretty_assertions::assert_eq!(params.timerel, Some(TimeRel::After));
        pretty_assertions::assert_eq!(params.time_at, Some("2024-01-01T00:00:00Z".parse().expect("valid instant")));
        pretty_assertions::assert_eq!(params.timeproperty, Some(TimeProp::Observed));
    }

    #[test]
    fn temporal_translates_last_n() {
        // Clause 5.7.3
        let mut q = base_query();
        q.temporal_q = Some(
            SnapshotTemporalQuery::builder()
                .timerel(TimeRel::Before)
                .time_at(instant("2024-12-31T23:59:59Z"))
                .end_time_at(None)
                .last_n(Some(42))
                .timeproperty(None)
                .aggr_methods(None)
                .aggr_period_duration(None)
                .build(),
        );
        let params = snapshot_query_to_temporal_params(&q).expect("translate");
        pretty_assertions::assert_eq!(params.last_n, Some(42));
    }

    #[test]
    fn temporal_translates_aggr_methods_and_period() {
        // Clause 5.7.3
        let mut q = base_query();
        q.temporal_q = Some(
            SnapshotTemporalQuery::builder()
                .timerel(TimeRel::Between)
                .time_at(instant("2024-01-01T00:00:00Z"))
                .end_time_at(Some(instant("2024-06-01T00:00:00Z")))
                .last_n(None)
                .timeproperty(None)
                .aggr_methods(Some(vec![AggrMethod::Avg, AggrMethod::Min]))
                .aggr_period_duration(AggregationPeriod::parse("PT1H").expect("valid period"))
                .build(),
        );
        let params = snapshot_query_to_temporal_params(&q).expect("translate");
        pretty_assertions::assert_eq!(params.aggr_methods, Some(vec![AggrMethod::Avg, AggrMethod::Min]));
        pretty_assertions::assert_eq!(params.aggr_period_duration.map(|period| period.as_iso8601()), Some("PT1H".to_string()));
        pretty_assertions::assert_eq!(params.end_time_at, Some("2024-06-01T00:00:00Z".parse().expect("valid instant")));
    }

    #[test]
    fn temporal_fails_when_temporal_q_missing() {
        // Clause 5.2.23: temporal queries require temporalQ
        let q = base_query();
        let err = snapshot_query_to_temporal_params(&q).expect_err("missing temporalQ");
        assert!(matches!(err, SnapshotQueryTranslationError::MissingTemporalQ));
    }
}
