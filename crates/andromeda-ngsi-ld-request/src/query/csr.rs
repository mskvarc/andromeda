//! Query parameters for the NGSI-LD Context Source Registration discovery API.

use crate::{
    error::ParameterError,
    impl_shared_query_params,
    language_filter::deserialize_optional_language_filter,
    options::{SysAttrsOptions, deserialize_sys_attrs_options},
    query::{
        SharedQueryParams,
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_selectors,
            deserialize_optional_filter_expr,
            deserialize_optional_pattern,
            deserialize_optional_projection_terms,
            deserialize_optional_uri_csv,
        },
    },
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp, time_rel::TimeRel, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    pattern::Pattern,
    projection::grammar::RawProjectionTerm,
    scope::query::ScopeQuery,
    temporal::window::TimeInterval,
    type_selection::grammar::TypeExprRaw,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Raw HTTP query parameters for Context Source Registration discovery
/// (Table 6.8.3.2-1).
///
/// All values are in their raw (compact) form before context expansion. The
/// parameters describe entities, as an entity query's do (clause 5.10.2.1); the
/// response is the registrations of the sources that may hold them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsrQueryParameters {
    /// Entity identifiers to select, comma-separated (`id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Uri>>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    #[serde(default, deserialize_with = "deserialize_optional_pattern")]
    pub id_pattern: Option<Pattern>,
    /// Attributes the registrations must declare, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` entity filter (clause 4.9), parsed at the request boundary.
    ///
    /// Clause 5.10.2.4: the attributes it names are matched against the
    /// registrations' `RegistrationInfo` like the `attrs` list; it is not
    /// evaluated against the registrations themselves.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// The context source filter (clause 4.9 `csf`), evaluated against the
    /// registrations' own Context Source Properties (clause 5.10.2.4).
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub csf: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at URL extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,
    /// The scope query matched against the registrations' `scope` (clause 4.19 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// Dataset instances the source must provide, comma-separated (`datasetId`;
    /// clause 5.12 `datasetId` rule).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Clause-4.15 natural-language selection preferences (`lang`).
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// The `GeoProperty` a `GeoJSON` response uses as its geometry (`geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// How the window relates to the given instants (clause 4.11 `timerel`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialization::deserialize_option_timerel")]
    pub timerel: Option<TimeRel>,
    /// The instant the window is anchored at (`timeAt`).
    #[serde(rename = "timeAt")]
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialization::deserialize_option_timeproperty")]
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
    id: id,
    attrs: attrs,
    pick: pick,
    omit: omit,
    dataset_id: dataset_id,
    limit: limit,
    offset: offset
});

impl CsrQueryParameters {
    /// Parses the `options` parameter into structured flags.
    #[must_use]
    pub fn parse_options(&self) -> SysAttrsOptions {
        self.options.unwrap_or_default()
    }

    /// Whether the query narrows the registrations at all.
    ///
    /// Clause 5.10.2.4 admits exactly four narrowing inputs: the type selector,
    /// the attribute list, the `q` query and the geoquery. Entity identifiers
    /// and an `idPattern` on their own are too wide (clause 5.10.2.3), and so
    /// are `csf`, `scopeQ` and the temporal window.
    #[must_use]
    pub const fn has_filter(&self) -> bool {
        self.entity_type.is_some() || self.attrs.is_some() || self.q.is_some() || self.geo.0.is_some()
    }

    /// Validates the CSR query parameters for completeness and correctness.
    ///
    /// At least one of the clause 5.10.2.4 narrowing parameters must be
    /// present, and the temporal parameter group must be complete when any
    /// member is specified.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::SelectorRequired`] when none of `type`,
    /// `attrs`, `q` and `georel` narrows the registrations, and
    /// [`ParameterError::Query`] when the `timerel`, `timeAt` and `endTimeAt`
    /// group is incomplete for the requested temporal relation.
    pub fn validate(&self) -> Result<(), ParameterError> {
        if !self.has_filter() {
            return Err(ParameterError::SelectorRequired("At least one of type, attrs, q, or georel must be provided"));
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

    /// Clause 5.10.2.4: `type`, `attrs`, `q` and the geoquery each narrow the
    /// query on their own.
    #[test]
    fn each_of_the_four_narrowing_parameters_satisfies_the_filter_rule() {
        for query in [
            "type=Vehicle",
            "attrs=speed",
            "q=speed>10",
            "georel=near%3BmaxDistance==100&geometry=Point&coordinates=[1,2]",
        ] {
            let p = params_from_query(query);
            assert!(p.has_filter(), "{query} must count as a filter");
            assert!(p.validate().is_ok(), "{query} must validate");
        }
    }

    /// Clause 5.10.2.3: identifiers alone are too wide, and so are the members
    /// clause 5.10.2.4 does not list among the narrowing inputs.
    #[test]
    fn identifiers_csf_scope_and_temporal_parameters_alone_do_not_satisfy_the_filter_rule() {
        for query in [
            "id=urn:ngsi-ld:Vehicle:1",
            "idPattern=urn:ngsi-ld:Vehicle:.*",
            "id=urn:ngsi-ld:Vehicle:1&idPattern=urn:.*",
            "csf=tier==%22gold%22",
            "scopeQ=/Madrid",
            "timerel=before&timeAt=2024-01-01T00:00:00Z",
            "datasetId=urn:ngsi-ld:dataset:1",
        ] {
            let p = params_from_query(query);
            assert!(!p.has_filter(), "{query} must not count as a filter");
            assert!(matches!(p.validate(), Err(ParameterError::SelectorRequired(_))), "{query} must be rejected");
        }
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

    /// Table 6.8.3.2-1: the members the entity query shares with discovery.
    #[test]
    fn the_table_6_8_3_2_1_members_are_read() {
        let p = params_from_query(
            "type=Vehicle&csf=tier==%22gold%22&scopeQ=/Madrid&datasetId=urn:ngsi-ld:dataset:1,@none&pick=id,speed&omit=type&lang=en&geometryProperty=location",
        );
        assert!(p.csf.is_some());
        assert!(p.scope_q.is_some());
        pretty_assertions::assert_eq!(p.dataset_ids().map(<[DatasetSelector]>::len), Some(2));
        pretty_assertions::assert_eq!(p.pick().map(<[RawProjectionTerm]>::len), Some(2));
        pretty_assertions::assert_eq!(p.omit().map(<[RawProjectionTerm]>::len), Some(1));
        assert!(p.lang.is_some());
        pretty_assertions::assert_eq!(p.geometry_property.as_ref().map(AttributeName::as_str), Some("location"));
    }

    #[test]
    fn an_unknown_parameter_is_rejected() {
        assert!(serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&bogus=1").is_err());
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
