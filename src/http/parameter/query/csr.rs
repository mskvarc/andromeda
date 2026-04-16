//! Query parameters for the NGSI-LD Context Source Registration discovery API.

use crate::{http::parameter::query::is_valid_uri, impl_shared_query_params, query::error::QueryError, renderer::SysAttrsInclusion};
use serde::Deserialize;

/// Raw HTTP query parameters for Context Source Registration discovery.
///
/// Corresponds to the query parameters defined in NGSI-LD 6.3.14 (Discover CSRs).
/// All values are in their raw (compact) form before context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsrQueryParameters {
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
    #[serde(rename = "geoproperty")]
    pub _geoproperty: Option<String>,
    pub timerel: Option<String>,
    #[serde(rename = "timeAt")]
    pub time_at: Option<String>,
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub options: Option<String>,
}

impl_shared_query_params!(CsrQueryParameters {
    id: id, attrs: attrs,
    pick: , omit: ,
    dataset_id: , limit: limit, offset: offset
});

impl CsrQueryParameters {
    /// Parses the `options` parameter into structured flags.
    pub fn parse_options(&self) -> CsrQueryParametersOptions {
        let opts = self.options.as_deref().unwrap_or("");
        CsrQueryParametersOptions {
            sys_attrs: if opts.contains("sysAttrs") {
                SysAttrsInclusion::Include
            } else {
                SysAttrsInclusion::Strip
            },
        }
    }

    /// Validates the CSR query parameters for completeness and correctness.
    ///
    /// At least one filtering parameter must be present. Geo and temporal
    /// parameter groups must be complete when any member is specified.
    pub fn validate(&self) -> Result<(), QueryError> {
        // At least one filtering parameter is required
        let has_filter =
            self.entity_type.is_some() || self.id.is_some() || self.id_pattern.is_some() || self.attrs.is_some() || self.q.is_some() || self.georel.is_some();
        if !has_filter {
            return Err(QueryError::ParamOther(
                "At least one of type, id, idPattern, attrs, q, or georel must be provided".to_string(),
            ));
        }

        // Validate each comma-separated id is a valid URI
        if let Some(ref ids) = self.id {
            for id in ids.split(',').map(str::trim) {
                if !id.is_empty() && !is_valid_uri(id) {
                    return Err(QueryError::ParamIdInvalid(id.to_string()));
                }
            }
        }

        // Geo query requires georel, geometry, and coordinates together
        if (self.georel.is_some() || self.geometry.is_some() || self.coordinates.is_some())
            && (self.georel.is_none() || self.geometry.is_none() || self.coordinates.is_none())
        {
            return Err(QueryError::GeoIncomplete);
        }

        // Temporal query requires timeAt when timerel is present
        if self.timerel.is_some() && self.time_at.is_none() {
            return Err(QueryError::ParamTimerelTimeatPair);
        }

        // between requires endTimeAt
        if self.timerel.as_deref() == Some("between") && self.end_time_at.is_none() {
            return Err(QueryError::ParamEndTimeAtRequired);
        }

        Ok(())
    }
}

/// Parsed response shaping options from the CSR `options` parameter.
#[derive(Debug, Default)]
pub struct CsrQueryParametersOptions {
    /// Whether to include system-generated attributes (`createdAt`, `modifiedAt`).
    pub sys_attrs: SysAttrsInclusion,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::{http::parameter::query::SharedQueryParams, params_from_query};

    fn params_from_query(query: &str) -> CsrQueryParameters {
        params_from_query!(query, CsrQueryParameters)
    }

    #[test]
    fn validate_requires_at_least_one_filter() {
        let p = params_from_query("");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("At least one of"));
    }

    #[test]
    fn validate_accepts_type_filter() {
        let p = params_from_query("type=Vehicle");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn validate_rejects_invalid_id_uri() {
        let p = params_from_query("id=notavaliduri");
        let err = p.validate().expect_err("expected error");
        assert!(matches!(err, QueryError::ParamIdInvalid(_)));
    }

    #[test]
    fn validate_rejects_incomplete_geo() {
        let p = params_from_query("type=Vehicle&georel=within&geometry=Point");
        let err = p.validate().expect_err("expected error");
        assert!(matches!(err, QueryError::GeoIncomplete));
    }

    #[test]
    fn validate_rejects_temporal_without_time_at() {
        let p = params_from_query("type=Vehicle&timerel=before");
        let err = p.validate().expect_err("expected error");
        assert!(matches!(err, QueryError::ParamTimerelTimeatPair));
    }

    #[test]
    fn validate_rejects_between_without_end_time_at() {
        let p = params_from_query("type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("endTimeAt is required"));
    }

    #[test]
    fn parse_options_sys_attrs() {
        let p = params_from_query("type=Vehicle&options=sysAttrs");
        pretty_assertions::assert_eq!(p.parse_options().sys_attrs, SysAttrsInclusion::Include);
    }

    #[test]
    fn parse_options_default() {
        let p = params_from_query("type=Vehicle");
        pretty_assertions::assert_eq!(p.parse_options().sys_attrs, SysAttrsInclusion::Strip);
    }

    #[test]
    fn shared_params_limit_offset() {
        let p = params_from_query("type=Vehicle&limit=50&offset=10");
        pretty_assertions::assert_eq!(p.effective_limit(), 50);
        pretty_assertions::assert_eq!(p.effective_offset(), 10);
    }

    #[test]
    fn shared_params_defaults() {
        let p = params_from_query("type=Vehicle");
        pretty_assertions::assert_eq!(p.effective_limit(), 20);
        pretty_assertions::assert_eq!(p.effective_offset(), 0);
    }
}
