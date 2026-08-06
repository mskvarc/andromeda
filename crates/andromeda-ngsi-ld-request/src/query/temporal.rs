//! Query parameters for the NGSI-LD Temporal API.

use crate::{body::temporal_query::TemporalQueryRequest, error::ParameterError, impl_shared_query_params};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::error::QueryError;
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
    /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
    pub id: Option<String>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    pub attrs: Option<String>,
    /// The `q` filter expression (clause 4.9), in compacted form.
    pub q: Option<String>,
    /// The spatial relationship of the geo-query (clause 4.10 `georel`).
    pub georel: Option<String>,
    /// The GeoJSON geometry type of the geo-query's reference (`geometry`).
    pub geometry: Option<String>,
    /// The coordinates of the geo-query's reference geometry (`coordinates`).
    pub coordinates: Option<String>,
    /// The GeoProperty the geo-query applies to (`geoproperty`).
    pub geoproperty: Option<String>,

    // --- Temporal filters ---
    /// How the window relates to the given instants (clause 4.11 `timerel`).
    pub timerel: Option<String>,
    /// The instant the window is anchored at (`timeAt`).
    #[serde(rename = "timeAt")]
    pub time_at: Option<String>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<String>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    pub timeproperty: Option<String>,

    // --- Result shaping ---
    /// How many instances per attribute the response carries (`lastN`).
    #[serde(rename = "lastN")]
    pub last_n: Option<u32>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<String>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
    /// The aggregation methods, comma-separated (clause 4.5.19 `aggrMethods`).
    #[serde(rename = "aggrMethods")]
    pub aggr_methods: Option<String>,
    /// The aggregation bucket width, as an ISO 8601 duration (`aggrPeriodDuration`).
    #[serde(rename = "aggrPeriodDuration")]
    pub aggr_period_duration: Option<String>,

    // --- Pagination ---
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Whether the response reports the total number of matches (`count`).
    pub count: Option<bool>,
    /// The ordering terms, accepted for the spec's shape and not read.
    #[serde(rename = "orderBy")]
    pub _order_by: Option<String>,

    // --- Projection ---
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    pub pick: Option<String>,
    /// Members the response drops (clause 6.3.15 `omit`).
    pub omit: Option<String>,

    // --- Other ---
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    /// Parsed by the language-negotiation extractor; retained so `deny_unknown_fields` accepts `?lang=...`.
    #[serde(rename = "lang")]
    pub _lang: Option<String>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    pub local: Option<bool>,

    /// Whether the query creates an entity map for stable pagination (`entityMap`).
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    /// How long a created entity map lives, as an ISO 8601 duration (`entityMapLifetime`).
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
    /// Whether attributes split across Context Sources are merged after forwarding (`splitEntities`).
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,

    /// The scope query expression (clause 4.18 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    /// Attributes whose `q` values are expanded against the context (`expandValues`).
    #[serde(rename = "expandValues")]
    pub expand_values: Option<String>,
    /// Attributes whose `q` paths address JSON object keys (`jsonKeys`).
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
    pub fn parse_timerel(&self) -> Option<Result<TimeRel, ParameterError>> {
        self.timerel
            .as_deref()
            .map(|s| s.parse::<TimeRel>().map_err(|_| ParameterError::TimerelInvalid(s.to_string())))
    }

    /// Parses the `timeproperty` parameter into a `TimeProp` enum value.
    ///
    /// Defaults to `TimeProp::Observed` (`observedAt`) when the parameter is
    /// absent. An unrecognised value is rejected rather than silently defaulted:
    /// clause 4.8 names the temporal properties, and quietly falling back to
    /// `observedAt` would query a different axis than the client asked for and
    /// return wrong data. `parse_timerel` rejects an unknown `timerel` the same
    /// way, so the two are consistent.
    pub fn parse_timeproperty(&self) -> Result<TimeProp, ParameterError> {
        match self.timeproperty.as_deref() {
            None => Ok(TimeProp::default()),
            Some(s) => s.parse::<TimeProp>().map_err(|_| ParameterError::TimePropertyInvalid(s.to_string())),
        }
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
    pub fn parse_aggr_methods(&self) -> Result<Vec<AggrMethod>, ParameterError> {
        match self.aggr_methods.as_deref() {
            Some(s) => s
                .split(',')
                .map(|m| {
                    let m = m.trim();
                    m.parse::<AggrMethod>().map_err(|_| ParameterError::AggrMethodInvalid(m.to_string()))
                })
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
    pub fn validate(&self, require_temporal: bool) -> Result<(), ParameterError> {
        // Validate format parameter
        if let Some(f) = &self.format
            && !matches!(f.as_str(), "temporalValues" | "aggregatedValues")
        {
            return Err(ParameterError::FormatInvalid(f.clone()));
        }

        // Validate options parameter
        if let Some(opts) = &self.options {
            for opt in opts.split(',') {
                let opt = opt.trim();
                if !matches!(opt, "temporalValues" | "aggregatedValues" | "sysAttrs" | "keyValues" | "simplified" | "concise") {
                    return Err(ParameterError::OptionsInvalid(opt.to_string()));
                }
            }
        }

        // Reject an unrecognised timeproperty rather than silently querying the
        // observedAt axis (clause 4.8).
        self.parse_timeproperty()?;

        if require_temporal {
            if self.timerel.is_none() || self.time_at.is_none() {
                return Err(ParameterError::TimerelTimeatRequired);
            }
            // At least one entity selection criterion is required for temporal query
            if self.id.is_none() && self.entity_type.is_none() && self.id_pattern.is_none() && self.attrs.is_none() && self.q.is_none() {
                return Err(ParameterError::SelectorRequired(
                    "At least one of id, type, idPattern, attrs, or q is required for temporal query",
                ));
            }
        } else if self.timerel.is_some() != self.time_at.is_some() {
            return Err(ParameterError::Query(QueryError::ParamTimerelTimeatPair));
        }
        if self.timerel.as_deref() == Some("between") && self.end_time_at.is_none() {
            return Err(ParameterError::Query(QueryError::ParamEndTimeAtRequired));
        }
        if self.parse_format() == TemporalFormat::AggregatedValues && self.aggr_methods.is_none() {
            return Err(ParameterError::AggrMethodsRequired);
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
            scope_q: None,
            expand_values: None,
            json_keys: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{params_from_query, query::SharedQueryParams};

    fn params_from_query(query: &str) -> TemporalQueryParameters {
        params_from_query!(query, TemporalQueryParameters)
    }

    #[test]
    fn parse_timerel_before() {
        let p = params_from_query("timerel=before");
        let result = p.parse_timerel().unwrap();
        pretty_assertions::assert_eq!(result.unwrap(), TimeRel::Before);
    }

    #[test]
    fn parse_timerel_after() {
        let p = params_from_query("timerel=after");
        pretty_assertions::assert_eq!(p.parse_timerel().unwrap().unwrap(), TimeRel::After);
    }

    #[test]
    fn parse_timerel_between() {
        let p = params_from_query("timerel=between");
        pretty_assertions::assert_eq!(p.parse_timerel().unwrap().unwrap(), TimeRel::Between);
    }

    #[test]
    fn parse_timerel_invalid() {
        let p = params_from_query("timerel=invalid");
        assert!(p.parse_timerel().unwrap().is_err());
    }

    #[test]
    fn parse_timeproperty_default_is_observed_at() {
        let p = params_from_query("");
        pretty_assertions::assert_eq!(p.parse_timeproperty().unwrap(), TimeProp::Observed);
    }

    #[test]
    fn parse_timeproperty_created_at() {
        let p = params_from_query("timeproperty=createdAt");
        pretty_assertions::assert_eq!(p.parse_timeproperty().unwrap(), TimeProp::Created);
    }

    #[test]
    fn parse_timeproperty_rejects_unrecognized_value() {
        // Clause 4.8: `timeproperty` names the temporal axis. An unrecognised
        // value is a malformed request, not a silent fallback to observedAt --
        // a client's typo must not quietly query a different axis.
        let p = params_from_query("timeproperty=bogus");
        assert!(matches!(p.parse_timeproperty(), Err(ParameterError::TimePropertyInvalid(_))));
    }

    #[test]
    fn validate_rejects_unrecognized_timeproperty() {
        let p = params_from_query("type=Vehicle&timerel=before&timeAt=2024-01-01T00:00:00Z&timeproperty=bogus");
        assert!(matches!(p.validate(true), Err(ParameterError::TimePropertyInvalid(_))));
    }

    #[test]
    fn parse_format_precedence_over_options() {
        let p = params_from_query("format=aggregatedValues&options=temporalValues");
        pretty_assertions::assert_eq!(p.parse_format(), TemporalFormat::AggregatedValues);
    }

    #[test]
    fn parse_format_from_options() {
        let p = params_from_query("options=temporalValues");
        pretty_assertions::assert_eq!(p.parse_format(), TemporalFormat::TemporalValues);
    }

    #[test]
    fn parse_format_normalized_default() {
        let p = params_from_query("");
        pretty_assertions::assert_eq!(p.parse_format(), TemporalFormat::Normalized);
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
        pretty_assertions::assert_eq!(methods, vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max]);
    }

    #[test]
    fn parse_aggr_methods_empty_when_absent() {
        let p = params_from_query("");
        assert!(p.parse_aggr_methods().unwrap().is_empty());
    }

    #[test]
    fn effective_limit_defaults_to_20() {
        let p = params_from_query("");
        pretty_assertions::assert_eq!(p.effective_limit(), 20);
    }

    #[test]
    fn effective_limit_caps_at_1000() {
        let p = params_from_query("limit=5000");
        pretty_assertions::assert_eq!(p.effective_limit(), 1000);
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
        use crate::body::temporal_query::TemporalQueryRequest;

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
        use crate::body::temporal_query::{TemporalQ, TemporalQueryRequest};

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
        pretty_assertions::assert_eq!(params.timerel.as_deref(), Some("after"));
        pretty_assertions::assert_eq!(params.time_at.as_deref(), Some("2024-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(params.timeproperty.as_deref(), Some("observedAt"));
        pretty_assertions::assert_eq!(params.aggr_methods.as_deref(), Some("avg"));
        pretty_assertions::assert_eq!(params.aggr_period_duration.as_deref(), Some("PT1H"));
        pretty_assertions::assert_eq!(params.attrs.as_deref(), Some("speed"));
        pretty_assertions::assert_eq!(params.limit, Some(50));
        pretty_assertions::assert_eq!(params.count, Some(true));
        pretty_assertions::assert_eq!(params.format.as_deref(), Some("temporalValues"));
        pretty_assertions::assert_eq!(params.last_n, Some(10));
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    #[test]
    fn from_temporal_query_request_without_temporal_q() {
        use crate::body::temporal_query::TemporalQueryRequest;

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
