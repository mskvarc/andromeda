//! Query parameters and compiled representation for the NGSI-LD Temporal API.

use crate::{
    domain::temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel},
    http::model::temporal_query_request::TemporalQueryRequest,
    query::{
        compiled::{GeoQuery, IdPattern},
        error::QueryError,
        q::ast::Expr,
    },
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Raw HTTP query parameters for temporal entity queries.
///
/// Corresponds to the query parameters defined in NGSI-LD 6.3.10 (Query Temporal
/// Evolution of Entities) and 5.7.3 (Retrieve Temporal Representation of an Entity).
/// All values are in their raw (compact) form before context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalQueryParameters {
    // --- Entity filters ---
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    pub attrs: Option<String>,
    pub q: Option<String>,
    pub georel: Option<String>,
    pub geometry: Option<String>,
    pub coordinates: Option<String>,
    pub geoproperty: Option<String>,

    // --- Temporal filters ---
    pub timerel: Option<String>,
    #[serde(rename = "timeAt")]
    pub time_at: Option<String>,
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,

    // --- Result shaping ---
    #[serde(rename = "lastN")]
    pub last_n: Option<u32>,
    pub format: Option<String>,
    pub options: Option<String>,
    #[serde(rename = "aggrMethods")]
    pub aggr_methods: Option<String>,
    #[serde(rename = "aggrPeriodDuration")]
    pub aggr_period_duration: Option<String>,

    // --- Pagination ---
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
    #[serde(rename = "orderBy")]
    pub _order_by: Option<String>,

    // --- Projection ---
    pub pick: Option<String>,
    pub omit: Option<String>,

    // --- Other ---
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "lang")]
    pub _lang: Option<String>,
    pub local: Option<bool>,

    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,

    #[serde(rename = "scopeQ")]
    pub _scope_q: Option<String>,
    #[serde(rename = "expandValues")]
    pub expand_values: Option<String>,
    #[serde(rename = "jsonKeys")]
    pub json_keys: Option<String>,
}

impl_shared_query_params!(TemporalQueryParameters {
    id: id,
    attrs: attrs,
    pick: pick,
    omit: omit,
    dataset_id: dataset_id,
    limit: limit,
    offset: offset
});

impl TemporalQueryParameters {
    /// Returns whether the query should be local-only (no distributed forwarding).
    ///
    /// Per NGSI-LD Table 6.18.3.2-1, `type=*` implies `local=true` since wildcard
    /// type queries are undefined across federated sources.
    pub fn is_local(&self) -> bool {
        if self.local == Some(true) {
            return true;
        }
        // type=* implies local=true per spec Table 6.18.3.2-1
        self.entity_type.as_deref() == Some("*")
    }

    /// Parses the `timerel` parameter into a `TimeRel` enum value.
    ///
    /// Returns `None` if `timerel` is absent, or `Some(Result)` with the parsed
    /// temporal relationship (`before`, `after`, `between`).
    pub fn parse_timerel(&self) -> Option<Result<TimeRel, QueryError>> {
        self.timerel
            .as_deref()
            .map(|s| s.parse::<TimeRel>().map_err(|e| QueryError::BadRequestData(e.to_string())))
    }

    /// Parses the `timeproperty` parameter into a `TimeProp` enum value.
    ///
    /// Defaults to `TimeProp::Observed` (`observedAt`) when the parameter is absent
    /// or contains an unrecognized value.
    pub fn parse_timeproperty(&self) -> TimeProp {
        self.timeproperty.as_deref().and_then(|s| s.parse::<TimeProp>().ok()).unwrap_or_default()
    }

    /// Parses the temporal response format from `format` and `options` parameters.
    ///
    /// The `format` parameter takes precedence over `options`. Recognized values
    /// are `"temporalValues"` and `"aggregatedValues"`. Defaults to
    /// `TemporalFormat::Normalized`.
    pub fn parse_format(&self) -> TemporalFormat {
        if let Some(f) = &self.format {
            match f.as_str() {
                "temporalValues" => return TemporalFormat::TemporalValues,
                "aggregatedValues" => return TemporalFormat::AggregatedValues,
                _ => {}
            }
        }
        if let Some(opts) = &self.options {
            if opts.contains("temporalValues") {
                return TemporalFormat::TemporalValues;
            }
            if opts.contains("aggregatedValues") {
                return TemporalFormat::AggregatedValues;
            }
        }
        TemporalFormat::Normalized
    }

    /// Parses the `aggrMethods` parameter as a comma-separated list of aggregation methods.
    ///
    /// Returns an empty vector when the parameter is absent.
    /// Returns an error if any method name is unrecognized.
    pub fn parse_aggr_methods(&self) -> Result<Vec<AggrMethod>, QueryError> {
        match self.aggr_methods.as_deref() {
            Some(s) => s
                .split(',')
                .map(|m| m.trim().parse::<AggrMethod>().map_err(|e| QueryError::BadRequestData(e.to_string())))
                .collect(),
            None => Ok(Vec::new()),
        }
    }

    /// Returns whether the `sysAttrs` option is present in the `options` parameter.
    pub fn has_sys_attrs(&self) -> bool {
        self.options.as_deref().is_some_and(|o| o.contains("sysAttrs"))
    }

    /// Returns whether the query requests simplified (key-values) output.
    ///
    /// Checks both `keyValues` and `simplified` option values, which are
    /// equivalent per NGSI-LD spec.
    pub fn is_simplified(&self) -> bool {
        self.options.as_deref().is_some_and(|o| o.contains("keyValues") || o.contains("simplified"))
    }

    /// Validate parameters.
    /// - For query endpoint (require_temporal=true): timerel and timeAt are REQUIRED.
    /// - For single-entity GET (require_temporal=false): timerel and timeAt are optional (both or neither).
    pub fn validate(&self, require_temporal: bool) -> Result<(), QueryError> {
        // Validate format parameter
        if let Some(f) = &self.format
            && !matches!(f.as_str(), "temporalValues" | "aggregatedValues")
        {
            return Err(QueryError::InvalidRequest(format!(
                "Invalid format value: '{}'. Must be 'temporalValues' or 'aggregatedValues'",
                f
            )));
        }

        // Validate options parameter
        if let Some(opts) = &self.options {
            for opt in opts.split(',') {
                let opt = opt.trim();
                if !matches!(opt, "temporalValues" | "aggregatedValues" | "sysAttrs" | "keyValues" | "simplified" | "concise") {
                    return Err(QueryError::InvalidRequest(format!("Invalid options value: '{}'", opt)));
                }
            }
        }

        if require_temporal {
            if self.timerel.is_none() || self.time_at.is_none() {
                return Err(QueryError::InvalidRequest("timerel and timeAt are required for temporal query".to_string()));
            }
            // At least one entity selection criterion is required for temporal query
            if self.id.is_none() && self.entity_type.is_none() && self.id_pattern.is_none() && self.attrs.is_none() && self.q.is_none() {
                return Err(QueryError::BadRequestData(
                    "At least one of id, type, idPattern, attrs, or q is required for temporal query".to_string(),
                ));
            }
        } else if self.timerel.is_some() != self.time_at.is_some() {
            return Err(QueryError::BadRequestData("timerel and timeAt must both be present or both absent".to_string()));
        }
        if self.timerel.as_deref() == Some("between") && self.end_time_at.is_none() {
            return Err(QueryError::BadRequestData("endTimeAt is required when timerel is 'between'".to_string()));
        }
        if self.parse_format() == TemporalFormat::AggregatedValues && self.aggr_methods.is_none() {
            return Err(QueryError::InvalidRequest(
                "aggrMethods is required when format is 'aggregatedValues'".to_string(),
            ));
        }
        Ok(())
    }
}

impl From<TemporalQueryRequest> for TemporalQueryParameters {
    fn from(request: TemporalQueryRequest) -> TemporalQueryParameters {
        let mut ids = Vec::new();
        let mut types = Vec::new();
        let mut id_pattern = None;

        if let Some(entities) = request.entities {
            for sel in entities {
                if let Some(id) = sel.id() {
                    ids.push(id.to_string());
                }
                if let Some(t) = sel.type_() {
                    types.push(t.to_string());
                }
                if let Some(p) = sel.id_pattern() {
                    id_pattern = Some(p.to_string());
                }
            }
        }

        let (timerel, time_at, end_time_at, timeproperty, aggr_methods, aggr_period_duration) = if let Some(tq) = request.temporal_q {
            (
                Some(tq.timerel),
                Some(tq.time_at),
                tq.end_time_at,
                tq.timeproperty,
                tq.aggr_methods,
                tq.aggr_period_duration,
            )
        } else {
            (None, None, None, None, None, None)
        };

        TemporalQueryParameters {
            id: if ids.is_empty() { None } else { Some(ids.join(",")) },
            entity_type: if types.is_empty() { None } else { Some(types.join(",")) },
            id_pattern,
            attrs: request.attrs.map(|a: Vec<String>| a.join(",")),
            q: request.q,
            georel: request.geo_q.as_ref().map(|g| g.georel.clone()), // clone: owned String from borrowed optional struct
            geometry: request.geo_q.as_ref().map(|g| g.geometry.clone()), // clone: owned String from borrowed optional struct
            coordinates: request.geo_q.as_ref().map(|g| g.coordinates.to_string()),
            geoproperty: request.geo_q.as_ref().and_then(|g| g.geoproperty.clone()), // clone: owned String from borrowed optional struct
            timerel,
            time_at,
            end_time_at,
            timeproperty,
            last_n: request.last_n,
            format: request.format,
            options: None,
            aggr_methods,
            aggr_period_duration,
            limit: request.limit,
            offset: request.offset,
            count: request.count,
            _order_by: None,
            pick: None,
            omit: None,
            dataset_id: None,
            _lang: None,
            local: request.local,
            entity_map: None,
            entity_map_lifetime: None,
            split_entities: None,
            _scope_q: None,
            expand_values: None,
            json_keys: None,
        }
    }
}

/// Indicates the origin of the `lastN` limit in a temporal query.
///
/// Indicates the origin of the `lastN` limit in a temporal query.
///
/// This enum replaces two boolean fields (`system_last_n` and
/// `user_specified_last_n`) to clearly distinguish between the four possible
/// states of lastN handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastNOrigin {
    /// The user explicitly specified a `lastN` parameter within the system cap.
    /// No truncation detection needed -- the user gets exactly what they asked for.
    UserSpecified,
    /// The user specified `lastN` but it exceeded the system cap, so the system
    /// reduced it. Triggers truncation detection (206 Partial Content) but uses
    /// user-specified direction for Content-Range.
    UserSpecifiedCapped,
    /// No user `lastN` was specified; the system applied DEFAULT_LAST_N as a cap.
    /// Triggers truncation detection (206 Partial Content) with system direction
    /// for Content-Range.
    SystemCapped,
    /// No `lastN` limit is active (e.g. aggregated queries without lastN).
    None,
}

impl LastNOrigin {
    /// Returns true when the system cap is active and truncation may occur.
    /// This is the case for both `SystemCapped` and `UserSpecifiedCapped`.
    pub const fn is_system_capped(self) -> bool {
        matches!(self, LastNOrigin::SystemCapped | LastNOrigin::UserSpecifiedCapped)
    }

    /// Returns true when the user explicitly provided a `lastN` parameter,
    /// regardless of whether it was further capped by the system.
    pub const fn is_user_specified(self) -> bool {
        matches!(self, LastNOrigin::UserSpecified | LastNOrigin::UserSpecifiedCapped)
    }
}

/// Compiled temporal query ready for SQL building.
///
/// Produced by expanding and validating `TemporalQueryParameters` against the
/// active JSON-LD context. Contains both entity-level filters and temporal-specific
/// parameters needed to query the `temporal_entity_attributes` and `attribute_history` tables.
#[derive(Debug, Clone)]
pub struct CompiledTemporalQuery {
    /// Optional set of entity IDs to filter by (expanded URIs/URNs).
    pub ids: Option<Vec<String>>,
    /// Optional regex pattern for matching entity IDs.
    pub id_pattern: Option<IdPattern>,
    /// Optional set of entity type IRIs to filter by.
    pub types: Option<Vec<String>>,
    /// Parsed `q` expression for property-level filtering.
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified.
    pub geo: Option<GeoQuery>,

    /// Temporal relationship operator (`before`, `after`, `between`).
    pub timerel: Option<TimeRel>,
    /// Reference time point for the temporal filter.
    pub time_at: Option<DateTime<Utc>>,
    /// End time point for `between` queries.
    pub end_time_at: Option<DateTime<Utc>>,
    /// The time property to filter on (`observedAt`, `createdAt`, `modifiedAt`, `deletedAt`).
    pub timeproperty: TimeProp,

    /// Expanded attribute IRIs to filter temporal data by.
    pub attr_filter: Vec<String>,
    /// Dataset IDs to filter temporal data by. `"@none"` matches the default (null) dataset.
    pub dataset_id_filter: Vec<String>,

    /// Maximum number of temporal instances per attribute to return.
    pub last_n: Option<u32>,
    /// The origin of the `lastN` limit, distinguishing user-specified values
    /// from system-imposed caps (which trigger 206 Partial Content).
    pub last_n_origin: LastNOrigin,
    /// The temporal response format (normalized, temporalValues, or aggregatedValues).
    pub format: TemporalFormat,
    /// Aggregation methods to apply when `format` is `aggregatedValues`.
    pub aggr_methods: Vec<AggrMethod>,
    /// ISO 8601 duration for aggregation bucket size (e.g. `"PT1H"`).
    pub aggr_period_duration: Option<String>,

    /// Maximum number of entities to return.
    pub limit: u32,
    /// Number of entities to skip for pagination.
    pub offset: u32,
    /// Whether to include a total count of matching entities in the response.
    pub count: bool,

    /// When true, include system-generated attributes (`createdAt`, `modifiedAt`).
    pub sys_attrs: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::params::SharedQueryParams;

    fn params_from_query(query: &str) -> TemporalQueryParameters {
        params_from_query!(query, TemporalQueryParameters)
    }

    #[test]
    fn parse_timerel_before() {
        let p = params_from_query("timerel=before");
        let result = p.parse_timerel().unwrap();
        assert_eq!(result.unwrap(), TimeRel::Before);
    }

    #[test]
    fn parse_timerel_after() {
        let p = params_from_query("timerel=after");
        assert_eq!(p.parse_timerel().unwrap().unwrap(), TimeRel::After);
    }

    #[test]
    fn parse_timerel_between() {
        let p = params_from_query("timerel=between");
        assert_eq!(p.parse_timerel().unwrap().unwrap(), TimeRel::Between);
    }

    #[test]
    fn parse_timerel_invalid() {
        let p = params_from_query("timerel=invalid");
        assert!(p.parse_timerel().unwrap().is_err());
    }

    #[test]
    fn parse_timeproperty_default_is_observed_at() {
        let p = params_from_query("");
        assert_eq!(p.parse_timeproperty(), TimeProp::Observed);
    }

    #[test]
    fn parse_timeproperty_created_at() {
        let p = params_from_query("timeproperty=createdAt");
        assert_eq!(p.parse_timeproperty(), TimeProp::Created);
    }

    #[test]
    fn parse_format_precedence_over_options() {
        let p = params_from_query("format=aggregatedValues&options=temporalValues");
        assert_eq!(p.parse_format(), TemporalFormat::AggregatedValues);
    }

    #[test]
    fn parse_format_from_options() {
        let p = params_from_query("options=temporalValues");
        assert_eq!(p.parse_format(), TemporalFormat::TemporalValues);
    }

    #[test]
    fn parse_format_normalized_default() {
        let p = params_from_query("");
        assert_eq!(p.parse_format(), TemporalFormat::Normalized);
    }

    #[test]
    fn validate_between_requires_end_time_at() {
        let p = params_from_query("timerel=between&timeAt=2024-01-01T00:00:00Z");
        assert!(p.validate(false).is_err());
    }

    #[test]
    fn validate_between_with_end_time_at_ok() {
        let p = params_from_query("timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-01-02T00:00:00Z");
        assert!(p.validate(false).is_ok());
    }

    #[test]
    fn validate_aggregated_requires_aggr_methods() {
        let p = params_from_query("format=aggregatedValues");
        assert!(p.validate(false).is_err());
    }

    #[test]
    fn validate_aggregated_with_aggr_methods_ok() {
        let p = params_from_query("format=aggregatedValues&aggrMethods=avg");
        assert!(p.validate(false).is_ok());
    }

    #[test]
    fn parse_aggr_methods_multiple() {
        let p = params_from_query("aggrMethods=avg,min,max");
        let methods = p.parse_aggr_methods().unwrap();
        assert_eq!(methods, vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max]);
    }

    #[test]
    fn parse_aggr_methods_empty_when_absent() {
        let p = params_from_query("");
        assert!(p.parse_aggr_methods().unwrap().is_empty());
    }

    #[test]
    fn effective_limit_defaults_to_20() {
        let p = params_from_query("");
        assert_eq!(p.effective_limit(), 20);
    }

    #[test]
    fn effective_limit_caps_at_1000() {
        let p = params_from_query("limit=5000");
        assert_eq!(p.effective_limit(), 1000);
    }

    #[test]
    fn is_local_defaults_to_false() {
        let p = params_from_query("");
        assert!(!p.is_local());
    }

    #[test]
    fn is_local_true_when_explicitly_set() {
        let p = params_from_query("local=true");
        assert!(p.is_local());
    }

    #[test]
    fn is_local_false_when_explicitly_set() {
        let p = params_from_query("local=false");
        assert!(!p.is_local());
    }

    #[test]
    fn is_local_true_when_type_is_wildcard() {
        let p = params_from_query("type=*");
        assert!(p.is_local());
    }

    #[test]
    fn is_local_false_with_specific_type() {
        let p = params_from_query("type=Vehicle");
        assert!(!p.is_local());
    }

    #[test]
    fn is_simplified_with_key_values() {
        let p = params_from_query("options=keyValues");
        assert!(p.is_simplified());
    }

    #[test]
    fn is_simplified_with_simplified_option() {
        let p = params_from_query("options=simplified");
        assert!(p.is_simplified());
    }

    #[test]
    fn is_simplified_default_is_false() {
        let p = params_from_query("");
        assert!(!p.is_simplified());
    }

    #[test]
    fn from_temporal_query_request_empty() {
        use crate::http::model::temporal_query_request::TemporalQueryRequest;

        let request = TemporalQueryRequest {
            _type: "Query".to_string(),
            entities: None,
            attrs: None,
            q: None,
            geo_q: None,
            temporal_q: None,
            limit: None,
            offset: None,
            count: None,
            format: None,
            last_n: None,
            local: None,
        };
        let params: TemporalQueryParameters = request.into();
        assert!(params.id.is_none());
        assert!(params.entity_type.is_none());
        assert!(params.timerel.is_none());
        assert!(params.time_at.is_none());
    }

    #[test]
    fn from_temporal_query_request_with_temporal_q() {
        use crate::http::model::temporal_query_request::{TemporalQ, TemporalQueryRequest};

        let request = TemporalQueryRequest {
            _type: "Query".to_string(),
            entities: None,
            attrs: Some(vec!["speed".to_string()]),
            q: None,
            geo_q: None,
            temporal_q: Some(TemporalQ {
                timerel: "after".to_string(),
                time_at: "2024-01-01T00:00:00Z".to_string(),
                end_time_at: None,
                timeproperty: Some("observedAt".to_string()),
                aggr_methods: Some("avg".to_string()),
                aggr_period_duration: Some("PT1H".to_string()),
            }),
            limit: Some(50),
            offset: None,
            count: Some(true),
            format: Some("temporalValues".to_string()),
            last_n: Some(10),
            local: Some(true),
        };
        let params: TemporalQueryParameters = request.into();
        assert_eq!(params.timerel.as_deref(), Some("after"));
        assert_eq!(params.time_at.as_deref(), Some("2024-01-01T00:00:00Z"));
        assert_eq!(params.timeproperty.as_deref(), Some("observedAt"));
        assert_eq!(params.aggr_methods.as_deref(), Some("avg"));
        assert_eq!(params.aggr_period_duration.as_deref(), Some("PT1H"));
        assert_eq!(params.attrs.as_deref(), Some("speed"));
        assert_eq!(params.limit, Some(50));
        assert_eq!(params.count, Some(true));
        assert_eq!(params.format.as_deref(), Some("temporalValues"));
        assert_eq!(params.last_n, Some(10));
        assert_eq!(params.local, Some(true));
    }

    #[test]
    fn from_temporal_query_request_without_temporal_q() {
        use crate::http::model::temporal_query_request::TemporalQueryRequest;

        let request = TemporalQueryRequest {
            _type: "Query".to_string(),
            entities: None,
            attrs: None,
            q: None,
            geo_q: None,
            temporal_q: None,
            limit: None,
            offset: None,
            count: None,
            format: None,
            last_n: None,
            local: None,
        };
        let params: TemporalQueryParameters = request.into();
        assert!(params.timerel.is_none());
        assert!(params.time_at.is_none());
        assert!(params.end_time_at.is_none());
        assert!(params.timeproperty.is_none());
        assert!(params.aggr_methods.is_none());
        assert!(params.aggr_period_duration.is_none());
    }
}
