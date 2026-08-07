//! Query parameters for the NGSI-LD Temporal API.

use crate::{
    body::temporal_query::TemporalQueryRequest,
    error::ParameterError,
    impl_shared_query_params,
    query::{
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_urns,
            deserialize_optional_filter_expr,
            deserialize_optional_pattern,
            deserialize_optional_uri_csv,
        },
        entity_selector::EntitySelectorParams,
    },
};
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    temporal_format::TemporalFormat,
    time_prop::TimeProp,
    time_rel::TimeRel,
    uri::Uri,
};
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    pattern::Pattern,
    scope::query::ScopeQuery,
    temporal::{
        aggregation_period::{AggregationPeriod, optional},
        window::TimeInterval,
    },
    type_selection::grammar::TypeExprRaw,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, de::Error};
use urn_rs::Urn;

/// Deserialise the `timerel` query parameter as the [`TimeRel`] it names
/// (clause 4.11), rejecting an unknown relationship at the boundary.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is present and not one of
/// `before`, `after`, `between`.
pub(crate) fn deserialize_option_timerel<'de, D>(deserializer: D) -> Result<Option<TimeRel>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .parse::<TimeRel>()
            .map_err(|_| serde::de::Error::custom(format!("invalid timerel: '{value}'")))
    })
    .transpose()
}

/// Deserialise a required `timerel` (as a POST temporal body carries it) as the
/// [`TimeRel`] it names (clause 4.11), rejecting an unknown relationship.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is not one of `before`,
/// `after`, `between`.
pub(crate) fn deserialize_timerel<'de, D>(deserializer: D) -> Result<TimeRel, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse::<TimeRel>()
        .map_err(|_| serde::de::Error::custom(format!("invalid timerel: '{raw}'")))
}

/// Deserialise the `timeproperty` query parameter as the [`TimeProp`] axis it
/// names (clause 4.8), rejecting an unknown property at the boundary.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is present and not a defined
/// temporal property.
pub(crate) fn deserialize_option_timeproperty<'de, D>(deserializer: D) -> Result<Option<TimeProp>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .parse::<TimeProp>()
            .map_err(|_| serde::de::Error::custom(format!("invalid timeproperty: '{value}'")))
    })
    .transpose()
}

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
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Uri>>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    #[serde(default, deserialize_with = "deserialize_optional_pattern")]
    pub id_pattern: Option<Pattern>,
    /// Per-selector entity constraints from a POST temporal query body's
    /// `entities[]` (clause 5.2.11), each still raw. Empty for a GET request;
    /// when non-empty it is authoritative for compilation, and the flat fields
    /// above are a flattened summary of it. Never a query string parameter, so
    /// serde skips it and the POST body conversion populates it.
    #[serde(skip)]
    pub entity_selectors: Vec<EntitySelectorParams>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at URL extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,

    // --- Temporal filters ---
    /// How the window relates to the given instants (clause 4.11 `timerel`).
    #[serde(default, deserialize_with = "deserialize_option_timerel")]
    pub timerel: Option<TimeRel>,
    /// The instant the window is anchored at (`timeAt`).
    #[serde(rename = "timeAt")]
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "deserialize_option_timeproperty")]
    pub timeproperty: Option<TimeProp>,

    // --- Result shaping ---
    /// How many instances per attribute the response carries (`lastN`).
    #[serde(rename = "lastN")]
    pub last_n: Option<u32>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "deserialize_option_temporal_format")]
    pub format: Option<TemporalFormat>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
    /// The aggregation methods, comma-separated (clause 4.5.19 `aggrMethods`).
    #[serde(rename = "aggrMethods")]
    #[serde(default, deserialize_with = "deserialize_optional_aggr_methods")]
    pub aggr_methods: Option<Vec<AggrMethod>>,
    /// The aggregation bucket width, as an ISO 8601 duration (`aggrPeriodDuration`).
    #[serde(rename = "aggrPeriodDuration")]
    #[serde(default, with = "optional")]
    pub aggr_period_duration: Option<AggregationPeriod>,

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
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub pick: Option<Vec<AttributeName>>,
    /// Members the response drops (clause 6.3.15 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub omit: Option<Vec<AttributeName>>,

    // --- Other ---
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_urns")]
    pub dataset_id: Option<Vec<Urn>>,
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
    pub scope_q: Option<ScopeQuery>,
    /// Attributes whose `q` values are expanded against the context (`expandValues`).
    #[serde(rename = "expandValues")]
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub expand_values: Option<Vec<AttributeName>>,
    /// Attributes whose `q` paths address JSON object keys (`jsonKeys`).
    #[serde(rename = "jsonKeys")]
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub json_keys: Option<Vec<AttributeName>>,
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
        matches!(self.entity_type, Some(TypeExprRaw::Wildcard))
    }

    /// The temporal property the window applies to, defaulting to `observedAt`
    /// (clause 4.8) when the request named none. The value was validated as a
    /// [`TimeProp`] at the query boundary.
    pub fn timeproperty_or_default(&self) -> TimeProp {
        self.timeproperty.unwrap_or_default()
    }

    /// Parses the temporal response format from `format` and `options` parameters.
    ///
    /// The `format` parameter takes precedence over `options`. Recognized values
    /// are `"temporalValues"` and `"aggregatedValues"`. Defaults to
    /// `TemporalFormat::Normalized`.
    pub fn parse_format(&self) -> TemporalFormat {
        if let Some(format) = self.format {
            return format;
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
        // Validate options parameter
        if let Some(opts) = &self.options {
            for opt in opts.split(',') {
                let opt = opt.trim();
                if !matches!(opt, "temporalValues" | "aggregatedValues" | "sysAttrs" | "keyValues" | "simplified" | "concise") {
                    return Err(ParameterError::OptionsInvalid(opt.to_string()));
                }
            }
        }

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
        }
        // The `timerel`/`timeAt`/`endTimeAt` combination rule is owned by the
        // query crate, so the request seam asks rather than re-deriving it.
        TimeInterval::check_time_bounds(
            self.timerel.is_some(),
            self.time_at.is_some(),
            self.timerel == Some(TimeRel::Between),
            self.end_time_at.is_some(),
        )
        .map_err(ParameterError::Query)?;
        if self.parse_format() == TemporalFormat::AggregatedValues && self.aggr_methods.is_none() {
            return Err(ParameterError::AggrMethodsRequired);
        }
        Ok(())
    }
}

impl From<TemporalQueryRequest> for TemporalQueryParameters {
    fn from(request: TemporalQueryRequest) -> TemporalQueryParameters {
        let mut ids = Vec::new();
        let mut id_pattern = None;
        let mut entity_selectors = Vec::new();

        if let Some(entities) = &request.entities {
            for sel in entities {
                if let Some(id) = sel.id() {
                    ids.push(id.clone());
                }
                if let Some(p) = sel.id_pattern() {
                    id_pattern = Some(p.clone());
                }
                // The precise per-selector grouping (clause 5.2.11), kept
                // alongside the flattened summary the fields above form.
                entity_selectors.push(EntitySelectorParams {
                    id: sel.id().map(|id| vec![id.clone()]),
                    id_pattern: sel.id_pattern().cloned(),
                    type_: sel.type_().cloned(),
                });
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
            id: if ids.is_empty() { None } else { Some(ids) },
            entity_type: None,
            id_pattern,
            entity_selectors,
            attrs: request.attrs,
            q: request.q,
            geo: OptionalGeoQuery(request.geo_q.as_ref().map(|geo| geo.0.clone())),
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
    fn timerel_before() {
        pretty_assertions::assert_eq!(params_from_query("timerel=before").timerel, Some(TimeRel::Before));
    }

    #[test]
    fn timerel_after() {
        pretty_assertions::assert_eq!(params_from_query("timerel=after").timerel, Some(TimeRel::After));
    }

    #[test]
    fn timerel_between() {
        pretty_assertions::assert_eq!(params_from_query("timerel=between").timerel, Some(TimeRel::Between));
    }

    #[test]
    fn timerel_invalid_is_rejected_at_the_boundary() {
        // Clause 4.11: an unknown `timerel` is a malformed request. It is now
        // rejected while the query deserialises rather than by a later parse.
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("timerel=invalid").is_err());
    }

    #[test]
    fn timeproperty_default_is_observed_at() {
        pretty_assertions::assert_eq!(params_from_query("").timeproperty_or_default(), TimeProp::Observed);
    }

    #[test]
    fn timeproperty_created_at() {
        pretty_assertions::assert_eq!(params_from_query("timeproperty=createdAt").timeproperty, Some(TimeProp::Created));
    }

    #[test]
    fn timeproperty_rejects_unrecognized_value_at_the_boundary() {
        // Clause 4.8: `timeproperty` names the temporal axis. An unrecognised
        // value is a malformed request, not a silent fallback to observedAt --
        // a client's typo must not quietly query a different axis. It is rejected
        // while the query deserialises.
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("timeproperty=bogus").is_err());
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
        pretty_assertions::assert_eq!(p.aggr_methods, Some(vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max]));
    }

    #[test]
    fn parse_aggr_methods_empty_when_absent() {
        let p = params_from_query("");
        assert!(p.aggr_methods.is_none());
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
            attrs: Some(vec!["speed".parse().unwrap()]),
            q: None,
            geo_q: None,
            temporal_q: Some(TemporalQ {
                timerel: TimeRel::After,
                time_at: "2024-01-01T00:00:00Z".parse().expect("valid RFC 3339 instant"),
                end_time_at: None,
                timeproperty: Some(TimeProp::Observed),
                aggr_methods: Some(vec![AggrMethod::Avg]),
                aggr_period_duration: AggregationPeriod::parse("PT1H").expect("valid duration"),
            }),
            limit: Some(50),
            offset: None,
            count: Some(true),
            format: Some(TemporalFormat::TemporalValues),
            last_n: Some(10),
            local: Some(true),
        };
        let params: TemporalQueryParameters = request.into();
        pretty_assertions::assert_eq!(params.timerel, Some(TimeRel::After));
        pretty_assertions::assert_eq!(params.time_at, Some("2024-01-01T00:00:00Z".parse().expect("valid RFC 3339 instant")));
        pretty_assertions::assert_eq!(params.timeproperty, Some(TimeProp::Observed));
        pretty_assertions::assert_eq!(params.aggr_methods, Some(vec![AggrMethod::Avg]));
        pretty_assertions::assert_eq!(params.aggr_period_duration.map(|period| period.as_iso8601()), Some("PT1H".to_string()));
        pretty_assertions::assert_eq!(params.attrs.as_deref().map(|names| names[0].as_str()), Some("speed"));
        pretty_assertions::assert_eq!(params.limit, Some(50));
        pretty_assertions::assert_eq!(params.count, Some(true));
        pretty_assertions::assert_eq!(params.format, Some(TemporalFormat::TemporalValues));
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
/// Deserialise a temporal `format` query parameter at the request boundary.
pub(crate) fn deserialize_option_temporal_format<'de, D>(deserializer: D) -> Result<Option<TemporalFormat>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| match value.as_str() {
        "temporalValues" => Ok(TemporalFormat::TemporalValues),
        "aggregatedValues" => Ok(TemporalFormat::AggregatedValues),
        other => Err(serde::de::Error::custom(format!("invalid temporal format: {other}"))),
    })
    .transpose()
}

/// Deserialize a comma-separated `aggrMethods` value into validated methods.
pub(crate) fn deserialize_optional_aggr_methods<'de, D>(deserializer: D) -> Result<Option<Vec<AggrMethod>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| value.split(',').map(str::trim).map(|method| method.parse().map_err(Error::custom)).collect())
        .transpose()
}
