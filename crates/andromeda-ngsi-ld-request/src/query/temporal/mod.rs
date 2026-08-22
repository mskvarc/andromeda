//! Query parameters for the NGSI-LD Temporal API.

use crate::{
    attribute_selection::validate_attrs,
    error::ParameterError,
    impl_shared_query_params,
    language_filter::deserialize_optional_language_filter,
    linked_retrieval_consistency::{validate_filter_join_depth, validate_projection_join_depth},
    options::{TemporalRepresentationOptions, deserialize_temporal_representation_options},
    query::{
        SharedQueryParams,
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_selectors,
            deserialize_optional_filter_expr,
            deserialize_optional_iso8601_duration,
            deserialize_optional_pattern,
            deserialize_optional_projection_terms,
            deserialize_optional_uri_csv,
        },
        entity_selector::EntitySelectorParams,
        temporal::deserialization::{
            deserialize_option_temporal_format,
            deserialize_option_timeproperty,
            deserialize_option_timerel,
            deserialize_optional_aggr_methods,
        },
    },
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    temporal_format::TemporalFormat,
    time_prop::TimeProp,
    time_rel::TimeRel,
    uri::Uri,
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    ordering::parameters::OrderingParameters,
    pattern::Pattern,
    projection::grammar::RawProjectionTerm,
    scope::query::ScopeQuery,
    temporal::{
        aggregation_period::{AggregationPeriod, optional},
        window::TimeInterval,
    },
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, sys_attrs::SysAttrsInclusion};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde::Deserialize;
use std::num::NonZeroU32;

/// Conversion of the canonical clause-5.2.23 body into these parameters.
pub mod body_conversion;
/// Boundary deserialisers for the temporal-only parameters.
pub mod deserialization;

/// Raw HTTP query parameters for temporal entity queries.
///
/// Corresponds to the query parameters defined in NGSI-LD 5.7.4 (Query Temporal
/// Evolution of Entities) and 5.7.3 (Retrieve Temporal Representation of an Entity).
/// All values are in their raw (compact) form before context expansion.
#[derive(Debug, Default, Deserialize)]
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
    /// Typed temporal representation and system-attribute options (`options`).
    #[serde(default, deserialize_with = "deserialize_temporal_representation_options")]
    pub options: Option<TemporalRepresentationOptions>,
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
    /// Typed ordering semantics shared by GET and POST query bindings.
    #[serde(flatten)]
    pub ordering: OrderingParameters,

    // --- Projection ---
    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,

    // --- Linked temporal entities ---
    /// Whether the response carries the temporal evolution of the entities it
    /// links to, and how (clause 4.5.23 `join`, applied to the temporal API).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    ///
    /// Typed as a positive integer so `joinLevel=0` is rejected while the query
    /// deserialises: clause 4.5.23 admits no zero-hop join, and zero names the
    /// absence of one rather than a depth.
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the client already holds, which the traversal must
    /// not retrieve again (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Uri>>,

    // --- Other ---
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(rename = "lang")]
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    pub local: Option<bool>,

    /// Whether the query creates an entity map for stable pagination (`entityMap`).
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    /// How long a created entity map lives, as an ISO 8601 duration (`entityMapLifetime`).
    #[serde(rename = "entityMapLifetime")]
    #[serde(default, deserialize_with = "deserialize_optional_iso8601_duration")]
    pub entity_map_lifetime: Option<Iso8601Duration>,
    /// Whether attributes split across Context Sources are merged after forwarding (`splitEntities`).
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,

    /// The scope query expression (clause 4.18 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// The context source filter (clause 5.7.4.3): a clause 4.9 query over the
    /// properties that describe a Context Source, deciding which registrations
    /// the temporal query is forwarded to and propagated unchanged into any
    /// discovery it triggers (clause 5.7.4.4).
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub csf: Option<Expr>,
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
        self.format.or_else(|| self.options.and_then(|options| options.format)).unwrap_or_default()
    }

    /// Returns whether the `sysAttrs` option is present in the `options` parameter.
    pub fn has_sys_attrs(&self) -> bool {
        self.options.is_some_and(|options| options.sys_attrs == SysAttrsInclusion::Include)
    }

    /// Returns whether the query requests simplified (key-values) output.
    ///
    /// Checks both `keyValues` and `simplified` option values, which are
    /// equivalent per NGSI-LD spec.
    pub const fn is_simplified(&self) -> bool {
        false
    }

    /// Validate parameters.
    /// - For query endpoint (require_temporal=true): timerel and timeAt are REQUIRED.
    /// - For single-entity GET (require_temporal=false): timerel and timeAt are optional (both or neither).
    pub fn validate(&self, require_temporal: bool) -> Result<(), ParameterError> {
        validate_attrs(self.attrs.as_deref())?;
        // Clause 4.21: pick and omit select opposite ends of the same
        // projection, so a request naming both is contradictory rather than a
        // request the broker can merge. The temporal API inherits the rule
        // unchanged.
        if self.pick.is_some() && self.omit.is_some() {
            return Err(ParameterError::PickOmitExclusive);
        }
        if (self.pick.is_some() || self.omit.is_some()) && self.attrs.is_some() {
            return Err(ParameterError::PickOmitWithAttrs);
        }
        // Clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
        // temporal operations at all, raising an error where projection
        // attributes or filter conditions indicate one. Andromeda's join
        // extension accepts them instead, and holds them to the bound clause
        // 5.7.2.4 sets on the entity bindings: a nested projection term and a
        // `q` hop each reach only as far as the requested join walks.
        validate_projection_join_depth(self.pick.as_deref(), self.omit.as_deref(), self.join, self.join_level)?;
        validate_filter_join_depth(self.q.as_ref(), self.join, self.join_level)?;
        // Clause 4.5.23 nests a linked entity under the attribute instance that
        // names it. The simplified representation of clause 4.5.5 and the
        // aggregated one of clause 4.5.19 both collapse instances into value
        // arrays, leaving nothing to nest under, so `join=inline` has no
        // meaning there and is refused rather than silently ignored. A flat
        // join stays available: it puts the linked entities beside the roots
        // and needs no nesting site.
        if self.join == Some(JoinMode::Inline) {
            let format = self.parse_format();
            match format {
                TemporalFormat::TemporalValues | TemporalFormat::AggregatedValues => {
                    return Err(ParameterError::InlineJoinWithoutInstances(format));
                }
                TemporalFormat::Normalized => {}
            }
        }
        // Clause 5.7.4.4: ordering is defined over the local result only, and
        // only the entity member `id` can order a temporal query.
        if !self.ordering.order_by.is_empty() && !self.is_local() {
            return Err(ParameterError::OrderingRequiresLocalScope);
        }
        if let Some(term) = self.ordering.order_by.iter().find(|term| AsRef::<str>::as_ref(&term.attr) != "id") {
            return Err(ParameterError::TemporalOrderingNotById(term.attr.to_string()));
        }
        if require_temporal {
            if self.timerel.is_none() || self.time_at.is_none() {
                return Err(ParameterError::TimerelTimeatRequired);
            }
            // At least one entity selection criterion is required for temporal query
            if self.id.is_none()
                && self.entity_type.is_none()
                && self.id_pattern.is_none()
                && self.attrs.is_none()
                && self.q.is_none()
                && self.entity_selectors.is_empty()
            {
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

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        params_from_query,
        query::{SharedQueryParams, temporal::TemporalQueryParameters},
    };
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn params_from_query(query: &str) -> TemporalQueryParameters {
        params_from_query!(query, TemporalQueryParameters)
    }

    #[test]
    fn timeproperty_default_is_observed_at() {
        pretty_assertions::assert_eq!(params_from_query("").timeproperty_or_default(), TimeProp::Observed);
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

    /// Clause 4.21: `observation{temperature,humidity}` is one raw term
    /// naming the `observation` relationship with two linked-entity sub-terms.
    /// Mirrors the entity-query parsing test in `entity/parameters.rs`.
    #[test]
    fn pick_with_a_linked_entity_group_parses_to_one_term_with_two_children() {
        let p = params_from_query("pick=observation%7Btemperature%2Chumidity%7D");
        let terms = p.pick.as_deref().expect("pick present");
        pretty_assertions::assert_eq!(terms.len(), 1);
        pretty_assertions::assert_eq!(terms[0].name.as_str(), "observation");
        pretty_assertions::assert_eq!(
            terms[0].linked.iter().map(|term| term.name.as_str()).collect::<Vec<_>>(),
            vec!["temperature", "humidity"]
        );
        assert!(p.omit.is_none());
    }

    /// Clause 4.21: `pick` and `omit` name opposite ends of one projection.
    /// A temporal query carrying both is contradictory, and is rejected with
    /// the same error the entity query raises.
    #[test]
    fn pick_and_omit_together_are_rejected() {
        let p = params_from_query("pick=speed&omit=fuel");

        let err = p.validate(false).expect_err("pick and omit are mutually exclusive");

        assert!(matches!(err, ParameterError::PickOmitExclusive));
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn pick_or_omit_with_attrs_is_rejected() {
        for query in [
            "timerel=after&timeAt=2024-01-01T00:00:00Z&pick=speed&attrs=fuelLevel",
            "timerel=after&timeAt=2024-01-01T00:00:00Z&omit=speed&attrs=fuelLevel",
        ] {
            let err = params_from_query(query)
                .validate(false)
                .expect_err("projection and attrs are mutually exclusive");
            assert!(matches!(err, ParameterError::PickOmitWithAttrs));
        }
    }

    /// Clause 4.5.23 nests a linked entity under the attribute instance naming
    /// it. `temporalValues` (clause 4.5.5) collapses instances into value
    /// arrays, so there is no instance to nest under and the combination is
    /// refused rather than silently ignored.
    #[test]
    fn an_inline_join_over_temporal_values_is_rejected() {
        let p = params_from_query("join=inline&format=temporalValues");

        let err = p.validate(false).expect_err("inline join has no nesting site");

        assert!(matches!(err, ParameterError::InlineJoinWithoutInstances(TemporalFormat::TemporalValues)));
        assert!(err.to_string().contains("join=inline"));
    }

    /// The aggregated representation of clause 4.5.19 collapses instances the
    /// same way, and is refused the same way.
    #[test]
    fn an_inline_join_over_aggregated_values_is_rejected() {
        let p = params_from_query("join=inline&format=aggregatedValues&aggrMethods=avg");

        let err = p.validate(false).expect_err("inline join has no nesting site");

        assert!(matches!(err, ParameterError::InlineJoinWithoutInstances(TemporalFormat::AggregatedValues)));
    }

    /// The same combination is refused when the representation was named
    /// through `options` rather than `format`.
    #[test]
    fn an_inline_join_over_a_representation_named_by_options_is_rejected() {
        let p = params_from_query("join=inline&options=temporalValues");

        assert!(matches!(
            p.validate(false),
            Err(ParameterError::InlineJoinWithoutInstances(TemporalFormat::TemporalValues))
        ));
    }

    /// A flat join needs no nesting site: it puts the linked entities beside
    /// the roots, so it stays available in every representation.
    #[test]
    fn a_flat_join_is_accepted_in_every_representation() {
        for query in [
            "join=flat&format=temporalValues",
            "join=flat&format=aggregatedValues&aggrMethods=avg",
            "join=flat",
        ] {
            assert!(params_from_query(query).validate(false).is_ok(), "flat join rejected for {query}");
        }
    }

    /// Clauses 5.7.3.4 and 5.7.4.4 refuse Linked Entity retrieval on the
    /// temporal operations outright; the join extension bounds it instead, by
    /// the clause 5.7.2.4 rule the entity bindings use. A `q` hop therefore
    /// needs a retrieving join, and `@none` is not one (clause 4.5.23 makes it
    /// the parameter's default).
    #[test]
    fn a_traversing_filter_is_bounded_by_the_requested_join() {
        for query in [
            "type=Vehicle&q=isParkedIn%7Bcategory%7D",
            "type=Vehicle&join=%40none&q=isParkedIn%7Bcategory%7D",
        ] {
            let err = params_from_query(query)
                .validate(false)
                .expect_err("a traversing filter needs a retrieving join");
            assert!(matches!(err, ParameterError::FilterTraversalWithoutJoin), "got {err:?} for {query}");
        }

        let too_deep = params_from_query("type=Vehicle&join=flat&joinLevel=1&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
        let err = too_deep.validate(false).expect_err("two hops overspend a one-hop traversal");
        assert!(
            matches!(err, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {err:?}"
        );

        let deep_enough = params_from_query("type=Vehicle&join=flat&joinLevel=2&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
        assert!(deep_enough.validate(false).is_ok());
    }

    /// The projection half of the same bullet pair: a nested `pick` term
    /// describes a Linked Entity, so the temporal extension holds it to the
    /// join that retrieves one, exactly as the entity bindings do.
    #[test]
    fn a_nested_projection_is_bounded_by_the_requested_join() {
        let unrequested = params_from_query("type=Vehicle&pick=isParkedIn%7Bcategory%7D");
        let err = unrequested.validate(false).expect_err("a nested projection needs a retrieving join");
        assert!(matches!(err, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {err:?}");

        let too_deep = params_from_query("type=Vehicle&join=flat&joinLevel=1&pick=a%7Bb%7Bc%7D%7D");
        let err = too_deep.validate(false).expect_err("two levels overspend a one-hop traversal");
        assert!(
            matches!(err, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {err:?}"
        );

        assert!(
            params_from_query("type=Vehicle&join=flat&joinLevel=1&pick=isParkedIn%7Bcategory%7D")
                .validate(false)
                .is_ok()
        );
    }

    /// An inline join over the normalized representation is exactly what
    /// clause 4.5.23 describes, and is accepted.
    #[test]
    fn an_inline_join_over_the_normalized_representation_is_accepted() {
        assert!(params_from_query("join=inline").validate(false).is_ok());
    }

    /// Clause 4.5.23 admits only a positive `joinLevel`; zero names the absence
    /// of a join rather than a depth. The `NonZeroU32` field rejects it while
    /// the query deserialises, which is what turns it into a 400 rather than a
    /// silently ignored parameter.
    #[test]
    fn a_join_level_of_zero_is_rejected_at_the_query_boundary() {
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("join=flat&joinLevel=0").is_err());
    }

    /// The three clause 4.5.23 join parameters parse off the temporal query
    /// string under the exact names the spec spells them with.
    #[test]
    fn the_join_parameters_parse_under_their_spec_names() {
        let p = params_from_query("join=flat&joinLevel=3&containedBy=urn:ngsi-ld:T:1,urn:ngsi-ld:T:2");

        pretty_assertions::assert_eq!(p.join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(p.join_level.map(NonZeroU32::get), Some(3));
        pretty_assertions::assert_eq!(
            p.contained_by.as_deref().map(|ids| ids.iter().map(Uri::as_str).collect::<Vec<_>>()),
            Some(vec!["urn:ngsi-ld:T:1", "urn:ngsi-ld:T:2"])
        );
    }

    /// A temporal query that asks for no join leaves all three absent, so the
    /// handler never reaches the traversal.
    #[test]
    fn the_join_parameters_are_absent_by_default() {
        let p = params_from_query("");

        assert!(p.join.is_none());
        assert!(p.join_level.is_none());
        assert!(p.contained_by.is_none());
    }

    /// Clause 5.7.4.3 lists the context source filter among the temporal
    /// query's inputs; it parses under the same `csf` name and clause 4.9
    /// grammar the entity query uses, and is absent by default.
    #[test]
    fn the_context_source_filter_parses_as_a_clause_4_9_expression() {
        let p = params_from_query("type=Vehicle&csf=mode%3D%3Dinclusive");
        // The canonical clause 4.9 text spells a bare token as the string
        // literal it is.
        pretty_assertions::assert_eq!(p.csf.as_ref().map(ToString::to_string).as_deref(), Some("mode==\"inclusive\""));
        assert!(params_from_query("type=Vehicle").csf.is_none());
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("csf=%20").is_err());
    }

    /// Clause 5.7.4.4: `orderBy` needs the local scope, and only `id` orders
    /// a temporal query.
    #[test]
    fn ordering_needs_the_local_scope_and_only_id_orders_a_temporal_query() {
        let window = "type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z";
        assert!(matches!(
            params_from_query(&format!("{window}&orderBy=id")).validate(true),
            Err(ParameterError::OrderingRequiresLocalScope)
        ));
        assert!(matches!(
            params_from_query(&format!("{window}&local=true&orderBy=speed")).validate(true),
            Err(ParameterError::TemporalOrderingNotById(member)) if member == "speed"
        ));
        assert!(params_from_query(&format!("{window}&local=true&orderBy=id")).validate(true).is_ok());
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
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("options=keyValues").is_err());
    }

    #[test]
    fn is_simplified_with_simplified_option() {
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("options=simplified").is_err());
    }

    #[test]
    fn is_simplified_default_is_false() {
        let p = params_from_query("");
        assert!(!p.is_simplified());
    }
}
