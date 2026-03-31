//! Query parameters and compiled representation for the NGSI-LD Temporal API.

use crate::{
    domain::temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel},
    query::{compiled::GeoQuery, error::QueryError, params::SharedQueryParams, q::Expr},
};
use serde::Deserialize;

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
    #[allow(dead_code)]
    pub order_by: Option<String>,

    // --- Projection ---
    pub pick: Option<String>,
    pub omit: Option<String>,

    // --- Other ---
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[allow(dead_code)]
    pub lang: Option<String>,
    #[allow(dead_code)]
    pub local: Option<bool>,

    #[serde(rename = "scopeQ")]
    #[allow(dead_code)]
    pub scope_q: Option<String>,
    #[serde(rename = "expandValues")]
    pub expand_values: Option<String>,
    #[serde(rename = "jsonKeys")]
    pub json_keys: Option<String>,
}

impl SharedQueryParams for TemporalQueryParameters {
    fn raw_id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    fn raw_entity_type(&self) -> Option<&str> {
        self.entity_type.as_deref()
    }
    fn raw_attrs(&self) -> Option<&str> {
        self.attrs.as_deref()
    }
    fn raw_pick(&self) -> Option<&str> {
        self.pick.as_deref()
    }
    fn raw_omit(&self) -> Option<&str> {
        self.omit.as_deref()
    }
    fn raw_dataset_id(&self) -> Option<&str> {
        self.dataset_id.as_deref()
    }
    fn raw_limit(&self) -> Option<u32> {
        self.limit
    }
    fn raw_offset(&self) -> Option<u32> {
        self.offset
    }
}

impl TemporalQueryParameters {
    pub fn parse_timerel(&self) -> Option<Result<TimeRel, QueryError>> {
        self.timerel
            .as_deref()
            .map(|s| s.parse::<TimeRel>().map_err(|e| QueryError::BadRequestData(e.to_string())))
    }

    pub fn parse_timeproperty(&self) -> TimeProp {
        self.timeproperty.as_deref().and_then(|s| s.parse::<TimeProp>().ok()).unwrap_or_default()
    }

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

    pub fn parse_aggr_methods(&self) -> Result<Vec<AggrMethod>, QueryError> {
        self.aggr_methods
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(|m| m.trim().parse::<AggrMethod>().map_err(|e| QueryError::BadRequestData(e.to_string())))
                    .collect()
            })
            .unwrap_or(Ok(Vec::new()))
    }

    pub fn has_sys_attrs(&self) -> bool {
        self.options.as_deref().map(|o| o.contains("sysAttrs")).unwrap_or(false)
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

/// Compiled temporal query ready for SQL building.
#[derive(Debug)]
pub struct CompiledTemporalQuery {
    // Entity-level filters
    pub ids: Option<Vec<String>>,
    pub id_pattern: Option<String>,
    pub types: Option<Vec<String>>,
    pub q_expr: Option<Expr>,
    pub geo: Option<GeoQuery>,

    // Temporal filters
    pub timerel: Option<TimeRel>,
    pub time_at: Option<String>,
    pub end_time_at: Option<String>,
    pub timeproperty: TimeProp,

    // Attribute/dataset filtering (expanded IRIs)
    pub attr_filter: Vec<String>,
    pub dataset_id_filter: Vec<String>,

    // Result shaping
    pub last_n: Option<u32>,
    /// True when the system's DEFAULT_LAST_N cap caused last_n to be set or reduced.
    /// Only system-caused truncation triggers 206 Partial Content + Content-Range.
    pub system_last_n: bool,
    /// True when the user explicitly specified a lastN parameter.
    pub user_specified_last_n: bool,
    pub format: TemporalFormat,
    pub aggr_methods: Vec<AggrMethod>,
    pub aggr_period_duration: Option<String>,

    // Pagination
    pub limit: u32,
    pub offset: u32,
    pub count: bool,

    // Projection
    #[allow(dead_code)]
    pub pick: Vec<String>,
    #[allow(dead_code)]
    pub omit: Vec<String>,

    // Response options
    pub sys_attrs: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::temporal::{AggrMethod, TemporalFormat, TimeProp, TimeRel};

    fn params_from_query(query: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(query).unwrap()
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
        assert_eq!(p.parse_timeproperty(), TimeProp::ObservedAt);
    }

    #[test]
    fn parse_timeproperty_created_at() {
        let p = params_from_query("timeproperty=createdAt");
        assert_eq!(p.parse_timeproperty(), TimeProp::CreatedAt);
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
}
