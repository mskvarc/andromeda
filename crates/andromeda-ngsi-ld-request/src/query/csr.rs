//! Query parameters for the NGSI-LD Context Source Registration discovery API.

use crate::{
    error::ParameterError,
    impl_shared_query_params,
    options::{SysAttrsOptions, deserialize_sys_attrs_options},
    query::{
        SharedQueryParams,
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_filter_expr,
            deserialize_optional_pattern,
            deserialize_optional_uri_csv,
        },
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp, time_rel::TimeRel, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    pattern::Pattern,
    projection::grammar::RawProjectionTerm,
    temporal::window::TimeInterval,
    type_selection::grammar::TypeExprRaw,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Raw HTTP query parameters for Context Source Registration discovery.
///
/// Corresponds to the query parameters defined in NGSI-LD 6.3.14 (Discover CSRs).
/// All values are in their raw (compact) form before context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsrQueryParameters {
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
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at URL extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,
    /// How the window relates to the given instants (clause 4.11 `timerel`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialize_option_timerel")]
    pub timerel: Option<TimeRel>,
    /// The instant the window is anchored at (`timeAt`).
    #[serde(rename = "timeAt")]
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialize_option_timeproperty")]
    pub timeproperty: Option<TimeProp>,
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// The legacy representation selector, comma-separated (`options`).
    #[serde(default, deserialize_with = "deserialize_sys_attrs_options")]
    pub options: Option<SysAttrsOptions>,
}

impl_shared_query_params!(CsrQueryParameters {
    id: id, attrs: attrs,
    pick: , omit: ,
    dataset_id: , limit: limit, offset: offset
});

impl CsrQueryParameters {
    /// Parses the `options` parameter into structured flags.
    pub fn parse_options(&self) -> SysAttrsOptions {
        self.options.unwrap_or_default()
    }

    /// Validates the CSR query parameters for completeness and correctness.
    ///
    /// At least one filtering parameter must be present. Geo and temporal
    /// parameter groups must be complete when any member is specified.
    pub fn validate(&self) -> Result<(), ParameterError> {
        // At least one filtering parameter is required
        let has_filter =
            self.entity_type.is_some() || self.id.is_some() || self.id_pattern.is_some() || self.attrs.is_some() || self.q.is_some() || self.geo.0.is_some();
        if !has_filter {
            return Err(ParameterError::SelectorRequired(
                "At least one of type, id, idPattern, attrs, q, or georel must be provided",
            ));
        }

        // Geo query requires georel, geometry, and coordinates together
        // The `timerel`/`timeAt`/`endTimeAt` combination rule is owned by the
        // query crate, so the request seam asks rather than re-deriving it — a
        // check that had drifted here, rejecting `timerel` without `timeAt` but
        // silently accepting `timeAt` without `timerel`.
        TimeInterval::check_time_bounds(
            self.timerel.is_some(),
            self.time_at.is_some(),
            self.timerel == Some(TimeRel::Between),
            self.end_time_at.is_some(),
        )
        .map_err(ParameterError::Query)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{params_from_query, query::SharedQueryParams};
    use andromeda_ngsi_ld_query::error::QueryError;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

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
    fn deserialization_rejects_invalid_id_uri() {
        assert!(serde_urlencoded::from_str::<CsrQueryParameters>("id=notavaliduri").is_err());
    }

    #[test]
    fn deserialization_rejects_an_empty_segment_in_a_comma_separated_id_filter() {
        assert!(
            serde_urlencoded::from_str::<CsrQueryParameters>("id=urn:ngsi-ld:ContextSourceRegistration:one,,urn:ngsi-ld:ContextSourceRegistration:two")
                .is_err()
        );
    }

    #[test]
    fn validate_rejects_incomplete_geo() {
        assert!(serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&georel=within&geometry=Point").is_err());
    }

    #[test]
    fn validate_rejects_temporal_without_time_at() {
        let p = params_from_query("type=Vehicle&timerel=before");
        let err = p.validate().expect_err("expected error");
        assert!(matches!(err, ParameterError::Query(QueryError::ParamTimerelTimeatPair)));
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
