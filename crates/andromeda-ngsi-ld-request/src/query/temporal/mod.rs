//! Query parameters for the NGSI-LD Temporal API.

use crate::{
    impl_shared_query_params,
    language_filter::deserialize_optional_language_filter,
    options::TemporalRepresentationOptions,
    pagination::parameters::PageParameters,
    query::{
        SharedQueryParams,
        deserialization::{
            OptionalGeoQuery,
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_selectors,
            deserialize_optional_filter_expr,
            deserialize_optional_iso8601_duration,
            deserialize_optional_join_level,
            deserialize_optional_pattern,
            deserialize_optional_projection_terms,
            deserialize_optional_uri_csv,
        },
        entity_selector::EntitySelectorParams,
        temporal::deserialization::{deserialize_option_timeproperty, deserialize_option_timerel, deserialize_optional_aggr_methods},
    },
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::WrittenValue,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    duration::Iso8601Duration,
    identifier::Identifier,
    temporal_format::TemporalFormat,
    time::instant_serde,
    time_prop::TimeProp,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    defined_token::deserialize_optional_token,
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
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::{borrow::Cow, num::NonZeroU32};

/// Conversion of the canonical clause-5.2.23 body into these parameters.
pub mod body_conversion;
/// Reusing the query machinery to resolve a delete-by-query's entity set.
pub mod delete_conversion;
/// The parameters of the temporal history delete-by-query extension.
pub mod delete_parameters;
/// What a temporal history delete-by-query has to satisfy before it runs.
pub mod delete_validation;
/// Boundary deserialisers for the temporal-only parameters.
pub mod deserialization;
/// The temporal query for the instance each stream held just before an
/// instant.
pub mod latest_before;
/// The operations a temporal query parameter set is read for.
pub mod operation;
/// The URL parameters the POST binding carries beside its clause 5.2.23 body.
pub mod post_parameters;
/// The parameters of the single-entity temporal retrieval.
pub mod retrieve_parameters;
/// What a temporal query has to satisfy before it runs.
pub mod validation;

/// Raw HTTP query parameters for temporal entity queries.
///
/// Corresponds to the query parameters defined in NGSI-LD 5.7.4 (Query Temporal
/// Evolution of Entities) and 5.7.3 (Retrieve Temporal Representation of an Entity).
/// All values are in their raw (compact) form before context expansion.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalQueryParameters {
    /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Identifier>>,
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

    /// How the window relates to the given instants (clause 4.11 `timerel`).
    #[serde(default, deserialize_with = "deserialize_option_timerel")]
    pub timerel: Option<TimeRel>,
    /// The instant the window is anchored at (`timeAt`).
    #[serde(rename = "timeAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "deserialize_option_timeproperty")]
    pub timeproperty: Option<TimeProp>,

    /// How many instances per attribute the response carries (`lastN`), a
    /// "Positive integer", so `lastN=0` fails to decode as `BadRequestData`.
    #[serde(rename = "lastN")]
    pub last_n: Option<NonZeroU32>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<WrittenValue<TemporalFormat>>,
    /// Typed temporal representation and system-attribute options (`options`).
    pub options: Option<WrittenValue<TemporalRepresentationOptions>>,
    /// The aggregation methods, comma-separated (clause 4.5.19 `aggrMethods`).
    #[serde(rename = "aggrMethods")]
    #[serde(default, deserialize_with = "deserialize_optional_aggr_methods")]
    pub aggr_methods: Option<Vec<AggrMethod>>,
    /// The aggregation bucket width, as an ISO 8601 duration (`aggrPeriodDuration`).
    #[serde(rename = "aggrPeriodDuration")]
    #[serde(default, with = "optional")]
    pub aggr_period_duration: Option<AggregationPeriod>,
    /// The instant the aggregation periods are laid from (`aggrPeriodOrigin`),
    /// which only a retrieval's continuation carries: no query string of a
    /// query names it, so serde skips it and the retrieval sets it.
    #[serde(skip)]
    pub aggr_period_origin: Option<DateTime<Utc>>,
    /// The grouping applied within each aggregation period (`aggrGroupBy`).
    ///
    /// Non-standard, from the temporal aggregation grouping extension. Carried
    /// raw and parsed by the query crate, consistent with `q` and `orderBy`; the
    /// HTTP gate rejects it when the extension is disabled, so a compiled query
    /// only ever sees it under an enabled broker.
    #[serde(rename = "aggrGroupBy")]
    pub aggr_group_by: Option<String>,

    /// The page of entities asked for (clauses 5.5.9 and 6.3.10 `limit`,
    /// `offset` and `cursor`), decided by the broker's pagination policy. It
    /// pages the element dimension only, never the temporal window
    /// (`Content-Range`).
    #[serde(flatten)]
    pub page: PageParameters,
    /// Whether the response reports the total number of matches (`count`).
    pub count: Option<bool>,
    /// Typed ordering semantics shared by GET and POST query bindings.
    #[serde(flatten)]
    pub ordering: OrderingParameters,

    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,

    /// Whether the response carries the temporal evolution of the entities it
    /// links to, and how (clause 4.5.23 `join`, applied to the temporal API).
    #[serde(default, deserialize_with = "deserialize_optional_token")]
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    ///
    /// Typed as a positive integer so `joinLevel=0` is rejected while the query
    /// deserialises: clause 4.5.23 admits no zero-hop join, and zero names the
    /// absence of one rather than a depth.
    #[serde(rename = "joinLevel")]
    #[serde(default, deserialize_with = "deserialize_optional_join_level")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the client already holds, which the traversal must
    /// not retrieve again (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Identifier>>,

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

query_parameter_set!(
    TemporalQueryParameters {
        Id,
        Type,
        IdPattern,
        Attrs,
        Q,
        Timerel,
        TimeAt,
        EndTimeAt,
        Timeproperty,
        LastN,
        Format,
        Options,
        AggrMethods,
        AggrPeriodDuration,
        AggrGroupBy,
        Count,
        Pick,
        Omit,
        Join,
        JoinLevel,
        ContainedBy,
        DatasetId,
        Lang,
        Local,
        EntityMap,
        EntityMapLifetime,
        SplitEntities,
        ScopeQ,
        Csf,
        ExpandValues,
        JsonKeys
    } + OptionalGeoQuery
        + PageParameters
        + OrderingParameters
);

impl_shared_query_params!(TemporalQueryParameters {
    id: id,
    attrs: attrs,
    pick: pick,
    omit: omit,
    dataset_id: dataset_id
});

impl TemporalQueryParameters {
    /// The entity selectors the query is the union of (clause 5.2.33).
    #[must_use]
    pub fn selectors(&self) -> Cow<'_, [EntitySelectorParams]> {
        EntitySelectorParams::effective(&self.entity_selectors, self.id.as_deref(), self.id_pattern.as_ref(), self.entity_type.as_ref())
    }

    /// Ask the query of `entities` alone, entities its selection has already
    /// matched.
    ///
    /// The identifier list narrows the flat selector and every clause 5.2.33
    /// selector alike; their type and pattern axes stay, which each listed
    /// entity satisfies already, so the union selects exactly the list. They
    /// also keep the narrowing clause 5.7.2.4 requires, which an identifier
    /// list alone is not. The value, geo and scope filters stay, since they
    /// also decide which instances are read.
    pub fn select_only(&mut self, entities: Vec<Identifier>) {
        for selector in &mut self.entity_selectors {
            selector.id = Some(entities.clone()); // clone: each selector of the union holds its own copy of the list
        }
        self.id = Some(entities);
    }

    /// Read the query over `interval` instead of the window it was asked with.
    pub const fn set_interval(&mut self, interval: TimeInterval) {
        (self.timerel, self.time_at, self.end_time_at) = interval.parts();
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        pagination::cursor_request::CursorRequest,
        params_from_query,
        query::{
            entity_selector::EntitySelectorParams,
            temporal::{TemporalQueryParameters, operation::TemporalQueryOperation},
        },
    };
    use andromeda_linked_retrieval::admission::{LinkedMember, LinkedOperation, ReachingMember, TemporalJoin};
    use andromeda_ngsi_ld_lexicon::{
        identifier::Identifier,
        temporal_format::TemporalFormat,
        time::rendering::instant_text,
        time_prop::TimeProp,
        time_rel::TimeRel,
    };
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        error::QueryError,
        filter::serialize::serialize_attr_path,
        projection::exclusivity::ProjectionConflict,
        temporal::window::TimeInterval,
    };
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn params_from_query(query: &str) -> TemporalQueryParameters {
        params_from_query!(query, TemporalQueryParameters)
    }

    #[test]
    fn a_query_asked_of_listed_entities_narrows_every_selector_and_keeps_its_narrowing() {
        let mut params = params_from_query("type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:.*&q=speed>10&timerel=after&timeAt=2024-01-01T00:00:00Z");
        params.entity_selectors.push(EntitySelectorParams::default());
        params.entity_selectors.push(EntitySelectorParams::default());
        let listed: Vec<Identifier> = vec!["urn:ngsi-ld:Vehicle:A".parse().expect("a valid identifier")];
        params.select_only(listed.clone());
        pretty_assertions::assert_eq!(params.id.as_ref(), Some(&listed));
        assert!(params.entity_selectors.iter().all(|selector| selector.id.as_ref() == Some(&listed)));
        assert!(params.entity_type.is_some() && params.id_pattern.is_some() && params.q.is_some());
        assert!(
            params.validate(TemporalQueryOperation::Query, TemporalJoin::Spec).is_ok(),
            "{:?}",
            params.validate(TemporalQueryOperation::Query, TemporalJoin::Spec)
        );
    }

    #[test]
    fn a_query_read_over_another_interval_spells_it_in_its_window_parameters() {
        let mut params = params_from_query("type=Vehicle&timerel=before&timeAt=2024-02-01T00:00:00Z");
        let from = "2024-01-01T00:00:00Z".parse().expect("a valid instant");
        let to = "2024-02-01T00:00:00Z".parse().expect("a valid instant");
        params.set_interval(TimeInterval::Between { from, to });
        pretty_assertions::assert_eq!(
            (params.timerel, params.time_at, params.end_time_at),
            (Some(TimeRel::Between), Some(from), Some(to))
        );
    }

    #[test]
    fn timeproperty_default_is_observed_at() {
        pretty_assertions::assert_eq!(params_from_query("").timeproperty_or_default(), TimeProp::Observed);
    }

    #[test]
    fn parse_format_precedence_over_options() {
        let p = params_from_query("format=aggregatedValues&options=temporalValues");
        pretty_assertions::assert_eq!(p.parse_format().expect("a supported rendering"), TemporalFormat::AggregatedValues);
    }

    #[test]
    fn parse_format_from_options() {
        let p = params_from_query("options=temporalValues");
        pretty_assertions::assert_eq!(p.parse_format().expect("a supported rendering"), TemporalFormat::TemporalValues);
    }

    #[test]
    fn parse_format_normalized_default() {
        let p = params_from_query("");
        pretty_assertions::assert_eq!(p.parse_format().expect("a supported rendering"), TemporalFormat::Normalized);
    }

    #[test]
    fn validate_between_requires_end_time_at() {
        let p = params_from_query("timerel=between&timeAt=2024-01-01T00:00:00Z");
        assert!(p.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended).is_err());
    }

    /// Clause 4.6.3: a comma fraction separator is accepted in a request.
    #[test]
    fn a_comma_fraction_is_accepted_in_the_window_instants() {
        let p = params_from_query("timerel=between&timeAt=2020-08-01T12:00:00,0Z&endTimeAt=2020-08-01T12:00:01,5Z");
        pretty_assertions::assert_eq!(p.time_at.map(instant_text).as_deref(), Some("2020-08-01T12:00:00Z"));
        pretty_assertions::assert_eq!(p.end_time_at.map(instant_text).as_deref(), Some("2020-08-01T12:00:01.500Z"));
    }

    /// Clause 4.11: `endTimeAt` is only used with `between`, and a `between`
    /// closes after it opens; both the retrieval and the query refuse it.
    #[test]
    fn validate_refuses_a_window_naming_no_interval() {
        for (query, expected) in [
            (
                "timerel=before&timeAt=2024-01-02T00:00:00Z&endTimeAt=2024-01-03T00:00:00Z",
                "endTimeAt is only allowed",
            ),
            (
                "timerel=after&timeAt=2024-01-02T00:00:00Z&endTimeAt=2024-01-03T00:00:00Z",
                "endTimeAt is only allowed",
            ),
            (
                "timerel=between&timeAt=2024-01-02T00:00:00Z&endTimeAt=2024-01-02T00:00:00Z",
                "later than timeAt",
            ),
            (
                "timerel=between&timeAt=2024-01-02T00:00:00Z&endTimeAt=2024-01-01T00:00:00Z",
                "later than timeAt",
            ),
        ] {
            let p = params_from_query(&format!("type=Vehicle&{query}"));
            for operation in [TemporalQueryOperation::Retrieve, TemporalQueryOperation::Query] {
                let error = p.validate(operation, TemporalJoin::Extended).expect_err(query);
                assert!(matches!(error, ParameterError::Query(_)), "{query}: {error:?}");
                assert!(error.to_string().contains(expected), "{query}: {error}");
            }
        }
        // The query requires `timerel` and `timeAt` before it reads the window.
        let error = params_from_query("type=Vehicle&endTimeAt=2024-01-03T00:00:00Z")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("a lone endTimeAt");
        assert!(matches!(error, ParameterError::Query(QueryError::ParamEndTimeAtWithoutBetween)), "{error:?}");
    }

    #[test]
    fn validate_between_with_end_time_at_ok() {
        let p = params_from_query("timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-01-02T00:00:00Z");
        assert!(p.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended).is_ok());
    }

    #[test]
    fn validate_aggregated_requires_aggr_methods() {
        let p = params_from_query("format=aggregatedValues");
        assert!(p.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended).is_err());
    }

    #[test]
    fn validate_aggregated_with_aggr_methods_ok() {
        let p = params_from_query("format=aggregatedValues&aggrMethods=avg");
        assert!(p.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended).is_ok());
    }

    /// Tables 6.18.3.2-1 and 6.19.3.1-1: `aggrMethods` and `aggrPeriodDuration`
    /// are "only applicable if aggregatedValues is present in the format or
    /// options parameter", and clause 6.3.20 refuses either without it.
    #[test]
    fn the_aggregation_parameters_without_aggregated_values_are_refused() {
        const WINDOW: &str = "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z";
        for aggregation in [
            "aggrMethods=sum",
            "aggrPeriodDuration=PT1H",
            "format=temporalValues&aggrMethods=sum&aggrPeriodDuration=PT1H",
            "options=sysAttrs&aggrMethods=sum",
        ] {
            let p = params_from_query(&format!("{WINDOW}&{aggregation}"));
            for operation in [TemporalQueryOperation::Retrieve, TemporalQueryOperation::Query] {
                let err = p.validate(operation, TemporalJoin::Spec).expect_err(aggregation);
                assert!(matches!(err, ParameterError::AggregationWithoutAggregatedValues), "{aggregation}: {err:?}");
            }
        }
    }

    /// Clause 5.7.3: `format` wins over `options`, so a request whose `format`
    /// is temporalValues is answered as temporal values and its aggregation
    /// parameters are ignored, while `options=aggregatedValues` still counts as
    /// naming aggregated values for the Tables 6.18.3.2-1 and 6.19.3.1-1 rule.
    #[test]
    fn a_winning_format_ignores_the_aggregation_parameters_that_options_admits() {
        const WINDOW: &str = "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z";
        let ignored = params_from_query(&format!(
            "{WINDOW}&format=temporalValues&options=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=PT1H"
        ));
        pretty_assertions::assert_eq!(ignored.selected_format(), TemporalFormat::TemporalValues);
        let aggregated = params_from_query(&format!("{WINDOW}&format=aggregatedValues&options=temporalValues&aggrMethods=sum"));
        pretty_assertions::assert_eq!(aggregated.selected_format(), TemporalFormat::AggregatedValues);
    }

    #[test]
    fn options_naming_aggregated_values_under_a_winning_format_do_not_require_methods() {
        const WINDOW: &str = "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z";
        let admitted = params_from_query(&format!("{WINDOW}&format=temporalValues&options=aggregatedValues"));
        assert!(admitted.validate(TemporalQueryOperation::Query, TemporalJoin::Spec).is_ok());
        let required = params_from_query(&format!("{WINDOW}&format=aggregatedValues"));
        let error = required
            .validate(TemporalQueryOperation::Query, TemporalJoin::Spec)
            .expect_err("methods are required");
        assert!(matches!(error, ParameterError::AggrMethodsRequired), "{error:?}");
    }

    #[test]
    fn a_period_origin_without_a_period_is_refused_whatever_options_name() {
        const WINDOW: &str = "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z";
        let mut p = params_from_query(&format!("{WINDOW}&format=temporalValues&options=aggregatedValues"));
        p.aggr_period_origin = Some("2024-01-01T00:00:00Z".parse().expect("a valid instant"));
        let error = p
            .validate(TemporalQueryOperation::Query, TemporalJoin::Spec)
            .expect_err("an origin needs periods");
        assert!(matches!(error, ParameterError::AggrPeriodOriginWithoutPeriods), "{error:?}");
    }

    /// Either spelling of `aggregatedValues` admits both aggregation
    /// parameters.
    #[test]
    fn the_aggregation_parameters_beside_aggregated_values_are_admitted() {
        const WINDOW: &str = "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z";
        for aggregation in [
            "options=aggregatedValues&aggrMethods=sum",
            "format=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=PT1H",
            "options=aggregatedValues,sysAttrs&aggrMethods=sum,avg&aggrPeriodDuration=P1D",
            "format=temporalValues&options=aggregatedValues&aggrMethods=sum",
            "format=temporalValues&options=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=PT1H",
            "format=aggregatedValues&options=temporalValues&aggrMethods=sum",
        ] {
            let p = params_from_query(&format!("{WINDOW}&{aggregation}"));
            for operation in [TemporalQueryOperation::Retrieve, TemporalQueryOperation::Query] {
                let result = p.validate(operation, TemporalJoin::Spec);
                assert!(result.is_ok(), "{aggregation}: {result:?}");
            }
        }
    }

    /// A valid grouped request passes.
    #[test]
    fn a_grouped_query_with_methods_and_a_group_key_is_admitted() {
        assert!(
            params_from_query("format=groupedValues&aggrMethods=avg&aggrGroupBy=unitCode")
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                .is_ok()
        );
    }

    /// Grouping only shapes the grouped format, so `aggrGroupBy`
    /// without it is refused.
    #[test]
    fn aggr_group_by_without_the_grouped_format_is_rejected() {
        let err = params_from_query("format=aggregatedValues&aggrMethods=avg&aggrGroupBy=unitCode")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("grouping needs the grouped format");
        assert!(matches!(err, ParameterError::AggrGroupByWithoutGroupedValues), "got {err:?}");
    }

    /// The grouped format is an aggregation, so it needs `aggrMethods` (clause
    /// 4.5.19).
    #[test]
    fn the_grouped_format_requires_aggr_methods() {
        let err = params_from_query("format=groupedValues&aggrGroupBy=unitCode")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("the grouped format aggregates");
        assert!(matches!(err, ParameterError::AggrMethodsRequired), "got {err:?}");
    }

    /// A malformed `aggrGroupBy` is refused with its parse error.
    #[test]
    fn an_unparseable_aggr_group_by_is_rejected() {
        let err = params_from_query("format=groupedValues&aggrMethods=avg&aggrGroupBy=speed%3Bbogus")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("bogus is no bucket");
        assert!(matches!(err, ParameterError::AggrGroupByInvalid(_)), "got {err:?}");
    }

    /// Grouping by an attribute the request also aggregates is
    /// ambiguous, so it must be spelled out.
    #[test]
    fn grouping_by_an_aggregation_target_is_rejected() {
        let err = params_from_query("format=groupedValues&aggrMethods=avg&attrs=speed&aggrGroupBy=speed")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("a self-referential group key is ambiguous");
        assert!(matches!(err, ParameterError::AggrGroupByOnAggregatedAttribute(_)), "got {err:?}");
    }

    /// `lastN` and grouping shape the result along different axes and
    /// are not combined.
    #[test]
    fn last_n_with_grouping_is_rejected() {
        let err = params_from_query("format=groupedValues&aggrMethods=avg&aggrGroupBy=unitCode&lastN=5")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("lastN and grouping do not combine");
        assert!(matches!(err, ParameterError::LastNWithGrouping), "got {err:?}");
    }

    /// The grouped format collapses instances like the aggregated one, so an
    /// inline join has no nesting site.
    #[test]
    fn an_inline_join_over_grouped_values_is_rejected() {
        let err = params_from_query("join=inline&format=groupedValues&aggrMethods=avg&aggrGroupBy=unitCode")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("inline join has no nesting site");
        assert!(
            matches!(err, ParameterError::InlineJoinWithoutInstances(TemporalFormat::GroupedValues)),
            "got {err:?}"
        );
    }

    /// Clause 4.21: `observation{temperature,humidity}` is one raw term
    /// naming the `observation` relationship with two linked-entity sub-terms.
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

    /// No clause forbids `pick` beside `omit` on a temporal query.
    #[test]
    fn pick_and_omit_together_are_admitted() {
        let p = params_from_query("pick=speed&omit=fuel");
        assert!(p.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended).is_ok());
    }

    #[test]
    fn pick_or_omit_with_attrs_is_rejected() {
        for query in [
            "timerel=after&timeAt=2024-01-01T00:00:00Z&pick=speed&attrs=fuelLevel",
            "timerel=after&timeAt=2024-01-01T00:00:00Z&omit=speed&attrs=fuelLevel",
        ] {
            let err = params_from_query(query)
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                .expect_err("projection and attrs are mutually exclusive");
            assert!(matches!(err, ParameterError::Projection(ProjectionConflict::ProjectionWithAttrs)));
        }
    }

    /// Clause 4.5.23 nests a linked entity under the attribute instance naming
    /// it. `temporalValues` (clause 4.5.5) collapses instances into value
    /// arrays, so there is no instance to nest under and the combination is
    /// refused rather than silently ignored.
    #[test]
    fn an_inline_join_over_temporal_values_is_rejected() {
        let p = params_from_query("join=inline&format=temporalValues");

        let err = p
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("inline join has no nesting site");

        assert!(matches!(err, ParameterError::InlineJoinWithoutInstances(TemporalFormat::TemporalValues)));
        assert!(err.to_string().contains("join=inline"));
    }

    /// The aggregated representation of clause 4.5.19 collapses instances the
    /// same way, and is refused the same way.
    #[test]
    fn an_inline_join_over_aggregated_values_is_rejected() {
        let p = params_from_query("join=inline&format=aggregatedValues&aggrMethods=avg");

        let err = p
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("inline join has no nesting site");

        assert!(matches!(err, ParameterError::InlineJoinWithoutInstances(TemporalFormat::AggregatedValues)));
    }

    /// The same combination is refused when the representation was named
    /// through `options` rather than `format`.
    #[test]
    fn an_inline_join_over_a_representation_named_by_options_is_rejected() {
        let p = params_from_query("join=inline&options=temporalValues");

        assert!(matches!(
            p.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended),
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
            assert!(
                params_from_query(query)
                    .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                    .is_ok(),
                "flat join rejected for {query}"
            );
        }
    }

    /// Clauses 5.7.3.4 and 5.7.4.4 refuse Linked Entity retrieval on the
    /// temporal operations outright: a nesting, a hop and a retrieving `join`
    /// in a query body are each `BadRequestData`, and `@none` asks for nothing.
    #[test]
    fn the_spec_temporal_operations_refuse_every_linked_reference() {
        let query = TemporalQueryOperation::Query;
        for (raw, member) in [
            ("type=Vehicle&pick=isParkedIn%7Bcategory%7D", LinkedMember::Reaching(ReachingMember::Projection)),
            ("type=Vehicle&q=isParkedIn%7Bcategory%7D", LinkedMember::Reaching(ReachingMember::Filter)),
            ("type=Vehicle&join=flat", LinkedMember::Join),
        ] {
            let err = params_from_query(raw)
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Spec)
                .expect_err("no Linked Entity retrieval");
            assert!(
                matches!(err, ParameterError::LinkedRetrievalUnavailable { operation: LinkedOperation::RetrieveTemporal(TemporalJoin::Spec), member: m } if m == member),
                "got {err:?} for {raw}"
            );
        }
        let window = "type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z";
        assert!(params_from_query(&format!("{window}&join=%40none")).validate(query, TemporalJoin::Spec).is_ok());
        assert!(
            params_from_query(&format!("{window}&pick=speed&q=speed%3E1"))
                .validate(query, TemporalJoin::Spec)
                .is_ok()
        );
    }

    /// With the `temporal_linked_retrieval` extension the temporal operations
    /// bound a nesting and a hop by the clause 5.7.2.4 rule the entity
    /// bindings use. A `q` hop therefore
    /// needs a retrieving join, and `@none` is not one (clause 4.5.23 makes it
    /// the parameter's default).
    #[test]
    fn a_traversing_filter_is_bounded_by_the_requested_join() {
        for query in [
            "type=Vehicle&q=isParkedIn%7Bcategory%7D",
            "type=Vehicle&join=%40none&q=isParkedIn%7Bcategory%7D",
        ] {
            let err = params_from_query(query)
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                .expect_err("a traversing filter needs a retrieving join");
            assert!(matches!(err, ParameterError::FilterTraversalWithoutJoin), "got {err:?} for {query}");
        }

        let too_deep = params_from_query("type=Vehicle&join=flat&joinLevel=1&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
        let err = too_deep
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("two hops overspend a one-hop traversal");
        assert!(
            matches!(err, ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {err:?}"
        );

        let deep_enough = params_from_query("type=Vehicle&join=flat&joinLevel=2&q=isParkedIn%7BinDistrict%7Bzone%7D%7D");
        assert!(deep_enough.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended).is_ok());
    }

    /// The projection half of the same bullet pair: a nested `pick` term
    /// describes a Linked Entity, so the temporal extension holds it to the
    /// join that retrieves one, exactly as the entity bindings do.
    #[test]
    fn a_nested_projection_is_bounded_by_the_requested_join() {
        let unrequested = params_from_query("type=Vehicle&pick=isParkedIn%7Bcategory%7D");
        let err = unrequested
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("a nested projection needs a retrieving join");
        assert!(matches!(err, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {err:?}");

        let too_deep = params_from_query("type=Vehicle&join=flat&joinLevel=1&pick=a%7Bb%7Bc%7D%7D");
        let err = too_deep
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("two levels overspend a one-hop traversal");
        assert!(
            matches!(err, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {err:?}"
        );

        assert!(
            params_from_query("type=Vehicle&join=flat&joinLevel=1&pick=isParkedIn%7Bcategory%7D")
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                .is_ok()
        );
    }

    /// Table 6.5.3.1-1: `joinLevel` is "only applicable if join parameter is
    /// 'flat' or 'inline'", with the extension or without it.
    #[test]
    fn a_join_level_without_a_retrieving_join_is_refused() {
        for linked_retrieval in [TemporalJoin::Spec, TemporalJoin::Extended] {
            for raw in ["type=Vehicle&joinLevel=2", "type=Vehicle&join=%40none&joinLevel=2"] {
                let err = params_from_query(raw)
                    .validate(TemporalQueryOperation::Retrieve, linked_retrieval)
                    .expect_err(raw);
                assert!(matches!(err, ParameterError::JoinLevelWithoutJoin), "{linked_retrieval:?} {raw}: {err:?}");
            }
        }
    }

    /// With the extension the temporal operations take `containedBy` as the
    /// entity ones do: "Only applicable if joinLevel is present".
    #[test]
    fn a_contained_by_without_a_join_level_is_refused() {
        let err = params_from_query("type=Vehicle&join=flat&containedBy=urn:ngsi-ld:Person:1")
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
            .expect_err("containedBy without joinLevel");
        assert!(matches!(err, ParameterError::ContainedByWithoutJoinLevel), "{err:?}");
        assert!(
            params_from_query("type=Vehicle&join=flat&joinLevel=1&containedBy=urn:ngsi-ld:Person:1")
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                .is_ok()
        );
    }

    /// Table 6.18.3.2-1: `entityMapLifetime` is "Only applicable if entityMap
    /// is set to true", while the Entity Map creation always creates its map.
    #[test]
    fn a_query_refuses_an_entity_map_lifetime_without_entity_map_true() {
        const WINDOW: &str = "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z";
        for lifetime in ["entityMapLifetime=PT1H", "entityMap=false&entityMapLifetime=PT1H"] {
            let p = params_from_query(&format!("{WINDOW}&{lifetime}"));
            let err = p.validate(TemporalQueryOperation::Query, TemporalJoin::Spec).expect_err(lifetime);
            assert!(matches!(err, ParameterError::EntityMapLifetimeWithoutEntityMap), "{lifetime}: {err:?}");
            assert!(p.validate(TemporalQueryOperation::EntityMap, TemporalJoin::Spec).is_ok(), "{lifetime}");
        }
        assert!(
            params_from_query(&format!("{WINDOW}&entityMap=true&entityMapLifetime=PT1H"))
                .validate(TemporalQueryOperation::Query, TemporalJoin::Spec)
                .is_ok()
        );
    }

    /// An inline join over the normalized representation is exactly what
    /// clause 4.5.23 describes, and is accepted.
    #[test]
    fn an_inline_join_over_the_normalized_representation_is_accepted() {
        assert!(
            params_from_query("join=inline")
                .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Extended)
                .is_ok()
        );
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
            p.contained_by.as_deref().map(|ids| ids.iter().map(Identifier::as_str).collect::<Vec<_>>()),
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
        let p = params_from_query("type=Vehicle&csf=mode%3D%3D%22inclusive%22");
        pretty_assertions::assert_eq!(p.csf.as_ref().map(ToString::to_string).as_deref(), Some("mode==\"inclusive\""));
        assert!(params_from_query("type=Vehicle").csf.is_none());
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("csf=%20").is_err());
    }

    /// Clause 5.7.4.4 admits only `id` as a temporal ordering member, which
    /// these parameters do settle. Whether the execution is limited to the
    /// local scope is not: that turns on the Context Registry, so an `orderBy`
    /// passes validation with or without `local=true`.
    #[test]
    fn only_id_orders_a_temporal_query_whatever_scope_it_asked_for() {
        let window = "type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z";
        assert!(
            params_from_query(&format!("{window}&orderBy=id"))
                .validate(TemporalQueryOperation::Query, TemporalJoin::Extended)
                .is_ok()
        );
        assert!(matches!(
            params_from_query(&format!("{window}&local=true&orderBy=speed")).validate(TemporalQueryOperation::Query, TemporalJoin::Extended),
            Err(ParameterError::Query(QueryError::TemporalOrderingMember(member))) if serialize_attr_path(&member) == "speed"
        ));
        assert!(
            params_from_query(&format!("{window}&local=true&orderBy=id"))
                .validate(TemporalQueryOperation::Query, TemporalJoin::Extended)
                .is_ok()
        );
    }

    /// Table 6.18.3.2-1 gives `orderBy` as "The Entity member ("id") appended
    /// with an optional sorting style ("asc", "desc")", so a distance style is
    /// refused rather than accepted and left without effect.
    #[test]
    fn a_distance_sorting_style_does_not_order_a_temporal_query() {
        let window = "type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z";
        assert!(matches!(
            params_from_query(&format!("{window}&local=true&orderBy=id;dist-asc&orderFrom=%5B8%2C40%5D"))
                .validate(TemporalQueryOperation::Query, TemporalJoin::Extended),
            Err(ParameterError::Query(QueryError::TemporalOrderingDirection(_)))
        ));
    }

    /// Table 6.18.3.2-1 makes `collation` "part of Entity ordering", so a
    /// collation with no `orderBy` orders nothing and needs no local scope.
    #[test]
    fn a_collation_without_an_order_by_is_admissible_without_the_local_scope() {
        let window = "type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z";
        assert!(
            params_from_query(&format!("{window}&collation=de-u-co-phonebk"))
                .validate(TemporalQueryOperation::Query, TemporalJoin::Extended)
                .is_ok()
        );
    }

    /// Clause 5.7.4.4: "If a preferred collation setting is present and it does
    /// not conform to a valid ICU collation (see IETF RFC 6067) then an error
    /// of type `BadRequestData` shall be raised." The value never becomes a
    /// parameter at all, so the rejection happens while the query string is
    /// read.
    #[test]
    fn an_invalid_collation_is_refused_while_the_temporal_query_string_is_read() {
        for invalid in ["collation=de-u-nu-latn", "collation=not%20a%20collation", "collation=und-u-zz-nonsense"] {
            assert!(serde_urlencoded::from_str::<TemporalQueryParameters>(invalid).is_err(), "{invalid}");
        }
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("collation=de-u-co-phonebk").is_ok());
    }

    /// The temporal query reads its page of entities beside its window; a
    /// cursor is decoded with the parameters.
    #[test]
    fn the_page_is_read_beside_the_window() {
        let p = params_from_query("type=Vehicle&local=true&timerel=before&timeAt=2030-01-01T00:00:00Z&limit=2&cursor=ZQ");
        pretty_assertions::assert_eq!(p.page.limit, Some(2));
        pretty_assertions::assert_eq!(p.page.cursor, Some(CursorRequest::Continue(PageCursor::End)));
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

    /// Table 6.18.3.2-1: with `type=*`, local "shall not be explicitly set to
    /// false", at the query and the Entity Map it creates.
    #[test]
    fn a_wildcard_type_refuses_local_false() {
        const WINDOW: &str = "timerel=after&timeAt=2024-01-01T00:00:00Z";
        for operation in [TemporalQueryOperation::Query, TemporalQueryOperation::EntityMap] {
            let err = params_from_query(&format!("type=*&local=false&{WINDOW}"))
                .validate(operation, TemporalJoin::Spec)
                .expect_err("type=* contradicts local=false");
            assert!(matches!(err, ParameterError::WildcardTypeWithLocalFalse), "{operation:?}: {err:?}");
            for admitted in ["type=*", "type=*&local=true", "type=Vehicle&local=false"] {
                assert!(
                    params_from_query(&format!("{admitted}&{WINDOW}"))
                        .validate(operation, TemporalJoin::Spec)
                        .is_ok(),
                    "{operation:?}: {admitted}"
                );
            }
        }
    }

    /// Table 5.2.33-1: a `*` among the selectors of a POST body implies local
    /// scope for the whole request, so `local=false` beside it is refused.
    #[test]
    fn a_wildcard_among_several_body_selectors_refuses_local_false() {
        const WINDOW: &str = "timerel=after&timeAt=2024-01-01T00:00:00Z";
        let selector = |selection: &str| EntitySelectorParams {
            type_: Some(selection.parse().expect("a valid type selector")),
            ..EntitySelectorParams::default()
        };
        for operation in [TemporalQueryOperation::Query, TemporalQueryOperation::EntityMap] {
            for local in ["", "&local=true", "&local=false"] {
                let mut wildcard = params_from_query(&format!("{WINDOW}{local}"));
                wildcard.entity_selectors = vec![selector("Vehicle"), selector("*")];
                assert!(wildcard.is_local(), "{operation:?}: {local}");
                let validated = wildcard.validate(operation, TemporalJoin::Spec);
                if local == "&local=false" {
                    assert!(
                        matches!(validated, Err(ParameterError::WildcardTypeWithLocalFalse)),
                        "{operation:?}: {validated:?}"
                    );
                } else {
                    assert!(validated.is_ok(), "{operation:?}: {local}: {validated:?}");
                }
            }
            let mut named = params_from_query(&format!("{WINDOW}&local=false"));
            named.entity_selectors = vec![selector("Vehicle"), selector("Building")];
            assert!(!named.is_local());
            assert!(named.validate(operation, TemporalJoin::Spec).is_ok(), "{operation:?}");
        }
    }

    #[test]
    fn is_local_false_with_specific_type() {
        let p = params_from_query("type=Vehicle");
        assert!(!p.is_local());
    }

    #[test]
    fn is_simplified_with_key_values() {
        let entity_token: TemporalQueryParameters = serde_urlencoded::from_str("options=keyValues").expect("the parameter set decodes");
        assert!(entity_token.parse_format().is_err(), "Table 6.3.12-1 defines no keyValues token");
    }

    #[test]
    fn is_simplified_with_simplified_option() {
        let entity_token: TemporalQueryParameters = serde_urlencoded::from_str("options=simplified").expect("the parameter set decodes");
        assert!(entity_token.parse_format().is_err(), "Table 6.3.12-1 defines no simplified token");
    }
}
