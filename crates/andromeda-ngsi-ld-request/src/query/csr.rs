//! Query parameters for the NGSI-LD Context Source Registration discovery API.

use crate::{
    error::ParameterError,
    impl_shared_query_params,
    language_filter::deserialize_optional_language_filter,
    options::SysAttrsOptions,
    pagination::parameters::PageParameters,
    query::{
        SharedQueryParams,
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_filter_expr,
            deserialize_optional_pattern,
            deserialize_optional_projection_terms,
            deserialize_optional_uri_csv,
        },
    },
    query_narrowing::{AdmittedNarrowings, NarrowingInputs, require_narrowing},
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::{WrittenValue, supported_option},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::instant_serde, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
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
/// response is the registrations of the sources that may hold them. The table
/// defines no `datasetId`: matching a registration's datasets (clause 5.12)
/// belongs to the distributed operations, not to discovery, so the parameter is
/// one the operation does not define (clause 6.3.20).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsrQueryParameters {
    /// Entity identifiers to select, comma-separated (`id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Identifier>>,
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
    #[serde(rename = "timeAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialization::deserialize_option_timeproperty")]
    pub timeproperty: Option<TimeProp>,
    /// The page asked for (clauses 5.5.9 and 6.3.10 `limit`, `offset` and
    /// `cursor`), decided by the broker's pagination policy.
    #[serde(flatten)]
    pub page: PageParameters,
    /// Whether the response reports the total number of matching
    /// registrations (clause 6.3.13 `count`, which clause 4.12 grants this
    /// paginated operation).
    #[serde(default)]
    pub count: CountPolicy,
    /// The deprecated representation selector, comma-separated (`options`).
    pub options: Option<WrittenValue<SysAttrsOptions>>,
}

query_parameter_set!(
    CsrQueryParameters {
        Id,
        Type,
        IdPattern,
        Attrs,
        Q,
        Csf,
        ScopeQ,
        Pick,
        Omit,
        Lang,
        GeometryProperty,
        Timerel,
        TimeAt,
        EndTimeAt,
        Timeproperty,
        Count,
        Options
    } + OptionalGeoQuery
        + PageParameters
);

impl_shared_query_params!(CsrQueryParameters {
    id: id,
    attrs: attrs,
    pick: pick,
    omit: omit,
    dataset_id:
});

impl CsrQueryParameters {
    /// The system-attribute selection the `options` parameter made.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clause 6.3.11
    /// does not define (clause 6.3.20).
    pub fn parse_options(&self) -> Result<SysAttrsOptions, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }

    /// The clause 5.10.2.4 narrowing inputs this discovery carries.
    ///
    /// Clause 5.10.2.4 admits exactly four: the type selector, the attribute
    /// list, the `q` query and the geoquery. Entity identifiers and an
    /// `idPattern` on their own are too wide (clause 5.10.2.3), and so are
    /// `csf`, `scopeQ` and the temporal window.
    #[must_use]
    pub fn narrowing_inputs(&self) -> NarrowingInputs<'_> {
        NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::REGISTRATIONS)
            .entity_type(self.entity_type.as_ref())
            .attrs(self.attrs.as_deref())
            .q(self.q.as_ref())
            .geo(self.geo.0.as_ref())
            .build()
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
    /// group names no interval.
    pub fn validate(&self) -> Result<(), ParameterError> {
        // Clause 6.3.20: `sysAttrs` is the one value clause 6.3.11 defines for
        // this operation's `options`.
        self.parse_options()?;
        require_narrowing(&self.narrowing_inputs())?;
        // Clause 5.5.9: an opaque `cursor` (5.5.9.1) and an `offset` (5.5.9.2)
        // page a collection by different mechanisms and cannot both govern it.
        // The `timerel`/`timeAt`/`endTimeAt` combination rule is owned by the
        // interval, so the request seam builds it rather than re-deriving it.
        TimeInterval::from_parts(self.timerel, self.time_at, self.end_time_at).map_err(ParameterError::Query)?;

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
        assert!(err.to_string().contains("too wide query"), "{err}");
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
        ] {
            let p = params_from_query(query);
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

    /// Clause 4.6.3 admits at most six fractional-second digits.
    #[test]
    fn deserialization_rejects_an_instant_beyond_six_fractional_digits() {
        assert!(serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00.123456789Z").is_err());
        assert!(
            serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00.1234567Z")
                .is_err()
        );
        assert!(serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00.123456Z").is_ok());
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

    /// Clause 4.11 and Table 6.8.3.2-1: `endTimeAt` comes with `between` alone,
    /// and a `between` closes after it opens.
    #[test]
    fn validate_rejects_a_window_naming_no_interval() {
        let err = params_from_query("type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z")
            .validate()
            .expect_err("endTimeAt beside after");
        assert!(matches!(err, ParameterError::Query(QueryError::ParamEndTimeAtWithoutBetween)), "{err:?}");
        let err = params_from_query("type=Vehicle&timerel=between&timeAt=2024-02-01T00:00:00Z&endTimeAt=2024-01-01T00:00:00Z")
            .validate()
            .expect_err("an inverted between");
        assert!(matches!(err, ParameterError::Query(QueryError::ParamEndTimeAtNotAfterTimeAt)), "{err:?}");
    }

    /// Table 6.8.3.2-1: the members the entity query shares with discovery.
    #[test]
    fn the_table_6_8_3_2_1_members_are_read() {
        let p = params_from_query("type=Vehicle&csf=tier==%22gold%22&scopeQ=/Madrid&pick=id,speed&omit=type&lang=en&geometryProperty=location");
        assert!(p.csf.is_some());
        assert!(p.scope_q.is_some());
        pretty_assertions::assert_eq!(p.dataset_ids(), None);
        pretty_assertions::assert_eq!(p.pick().map(<[RawProjectionTerm]>::len), Some(2));
        pretty_assertions::assert_eq!(p.omit().map(<[RawProjectionTerm]>::len), Some(1));
        assert!(p.lang.is_some());
        pretty_assertions::assert_eq!(p.geometry_property.as_ref().map(AttributeName::as_str), Some("location"));
    }

    #[test]
    fn an_unknown_parameter_is_rejected() {
        assert!(serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&bogus=1").is_err());
    }

    /// Table 6.8.3.2-1 defines no `datasetId` (clause 6.3.20).
    #[test]
    fn a_dataset_id_is_unknown() {
        let message = serde_urlencoded::from_str::<CsrQueryParameters>("type=Vehicle&datasetId=@none")
            .expect_err("refused")
            .to_string();
        assert!(message.contains("unknown field"), "{message}");
    }

    #[test]
    fn parse_options_sys_attrs() {
        let p = params_from_query("type=Vehicle&options=sysAttrs");
        pretty_assertions::assert_eq!(p.parse_options().expect("a supported options value").sys_attrs, SysAttrsInclusion::Include);
    }

    #[test]
    fn parse_options_default() {
        let p = params_from_query("type=Vehicle");
        pretty_assertions::assert_eq!(p.parse_options().expect("a supported options value").sys_attrs, SysAttrsInclusion::Strip);
    }

    /// The discovery reads its page beside its selection.
    #[test]
    fn the_page_is_read_beside_the_selection() {
        let p = params_from_query("type=Vehicle&limit=50&offset=10");
        pretty_assertions::assert_eq!(p.page.limit, Some(50));
        pretty_assertions::assert_eq!(p.page.offset, Some(10));
        pretty_assertions::assert_eq!(p.page.cursor, None);
        pretty_assertions::assert_eq!(p.count, CountPolicy::Omit);
    }

    /// The discovery is paginated (clause 4.12), so it reads the clause 6.3.13
    /// `count`.
    #[test]
    fn the_count_is_read_beside_the_page() {
        let p = params_from_query("type=Vehicle&limit=0&count=true");
        pretty_assertions::assert_eq!(p.page.limit, Some(0));
        pretty_assertions::assert_eq!(p.count, CountPolicy::Emit);
    }

    /// Table 6.3.11-1 gives the discovery's `options` only `sysAttrs`.
    #[test]
    fn a_count_through_options_is_refused() {
        let p = params_from_query("type=Vehicle&options=count");
        assert!(matches!(p.parse_options(), Err(ParameterError::OptionsInvalid(_))));
    }
}
