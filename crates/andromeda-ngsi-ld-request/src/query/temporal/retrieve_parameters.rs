//! The parameters of Retrieve Temporal Evolution of an Entity (clause 5.7.3,
//! bound to `GET /temporal/entities/{entityId}` by clause 6.19.3.1).
//!
//! Table 6.19.3.1-1 lists the window (`timerel`, `timeAt`, `endTimeAt`,
//! `timeproperty`), `lastN`, the projection (`attrs`, `pick`, `omit`),
//! `datasetId`, `lang` and the aggregation (`aggrMethods`,
//! `aggrPeriodDuration`). Clauses 6.3.11 and 6.3.12 add `format` and
//! `options`, clause 6.3.18 adds `local`, and clause 5.7.3.3 adds the
//! `entityMap` flag and its suggested lifetime, which the table leaves out.
//! `aggrPeriodOrigin` is Andromeda's: the `rel="next"` continuation of a
//! calendar-grid aggregation carries it, as a collection pointer carries
//! `pageContext`, so the clause 6.3.10 next page lies on the grid of the first
//! (clause 4.5.19). The
//! `temporal_linked_retrieval` and `temporal_aggregation_grouping` extensions
//! add `join`, `joinLevel`, `containedBy` and `aggrGroupBy`, which the
//! binding's extension gate refuses while they are off. One entity's history
//! is no collection clause 4.12 pages, and the operation selects no entity, so
//! every other parameter of Query Temporal Evolution of Entities is one it does
//! not define (clause 6.3.20).

use crate::{
    language_filter::deserialize_optional_language_filter,
    options::TemporalRepresentationOptions,
    query::{
        deserialization::{
            deserialize_optional_attribute_names,
            deserialize_optional_dataset_selectors,
            deserialize_optional_iso8601_duration,
            deserialize_optional_join_level,
            deserialize_optional_projection_terms,
            deserialize_optional_uri_csv,
        },
        temporal::{
            TemporalQueryParameters,
            deserialization::{deserialize_option_timeproperty, deserialize_option_timerel, deserialize_optional_aggr_methods},
        },
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
    projection::grammar::RawProjectionTerm,
    temporal::aggregation_period::{AggregationPeriod, optional},
};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::num::NonZeroU32;

/// Raw parameters of one Retrieve Temporal Evolution of an Entity request, in
/// their compact form before context expansion.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalRetrieveParameters {
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
    /// How many instances per attribute the response carries (`lastN`).
    #[serde(rename = "lastN")]
    pub last_n: Option<NonZeroU32>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// The aggregation methods, comma-separated (clause 4.5.19 `aggrMethods`).
    #[serde(rename = "aggrMethods")]
    #[serde(default, deserialize_with = "deserialize_optional_aggr_methods")]
    pub aggr_methods: Option<Vec<AggrMethod>>,
    /// The aggregation bucket width, as an ISO 8601 duration (`aggrPeriodDuration`).
    #[serde(rename = "aggrPeriodDuration")]
    #[serde(default, with = "optional")]
    pub aggr_period_duration: Option<AggregationPeriod>,
    /// The instant the aggregation periods are laid from
    /// (`aggrPeriodOrigin`): the origin a `rel="next"` continuation of a
    /// calendar-grid aggregation keeps once its window opens past it, as the
    /// `pageContext` of a collection pointer keeps its context.
    #[serde(rename = "aggrPeriodOrigin", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub aggr_period_origin: Option<DateTime<Utc>>,
    /// The grouping within each aggregation period (`aggrGroupBy`, the
    /// `temporal_aggregation_grouping` extension).
    #[serde(rename = "aggrGroupBy")]
    pub aggr_group_by: Option<String>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<WrittenValue<TemporalFormat>>,
    /// Typed temporal representation and system-attribute options (`options`).
    pub options: Option<WrittenValue<TemporalRepresentationOptions>>,
    /// Whether the operation is confined to this broker (clause 6.3.18 `local`).
    pub local: Option<bool>,
    /// Whether the location of the `EntityMap` used is returned, one being
    /// created when none is in use (clause 5.7.3.3 `entityMap`).
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    /// The suggested lifetime of a created `EntityMap` (clause 5.7.3.3
    /// `entityMapLifetime`).
    #[serde(rename = "entityMapLifetime")]
    #[serde(default, deserialize_with = "deserialize_optional_iso8601_duration")]
    pub entity_map_lifetime: Option<Iso8601Duration>,
    /// How linked entities reach the response (clause 4.5.23 `join`, the
    /// `temporal_linked_retrieval` extension).
    #[serde(default, deserialize_with = "deserialize_optional_token")]
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    #[serde(default, deserialize_with = "deserialize_optional_join_level")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the client already holds (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Identifier>>,
}

query_parameter_set!(TemporalRetrieveParameters {
    Timerel,
    TimeAt,
    EndTimeAt,
    Timeproperty,
    LastN,
    Attrs,
    Pick,
    Omit,
    DatasetId,
    Lang,
    AggrMethods,
    AggrPeriodDuration,
    AggrPeriodOrigin,
    AggrGroupBy,
    Format,
    Options,
    Local,
    EntityMap,
    EntityMapLifetime,
    Join,
    JoinLevel,
    ContainedBy
});

impl From<TemporalRetrieveParameters> for TemporalQueryParameters {
    /// The retrieval as the temporal query model reads it: the members the
    /// retrieval does not define stay absent.
    fn from(retrieve: TemporalRetrieveParameters) -> TemporalQueryParameters {
        TemporalQueryParameters {
            timerel: retrieve.timerel,
            time_at: retrieve.time_at,
            end_time_at: retrieve.end_time_at,
            timeproperty: retrieve.timeproperty,
            last_n: retrieve.last_n,
            attrs: retrieve.attrs,
            pick: retrieve.pick,
            omit: retrieve.omit,
            dataset_id: retrieve.dataset_id,
            lang: retrieve.lang,
            aggr_methods: retrieve.aggr_methods,
            aggr_period_duration: retrieve.aggr_period_duration,
            aggr_period_origin: retrieve.aggr_period_origin,
            aggr_group_by: retrieve.aggr_group_by,
            format: retrieve.format,
            options: retrieve.options,
            local: retrieve.local,
            entity_map: retrieve.entity_map,
            entity_map_lifetime: retrieve.entity_map_lifetime,
            join: retrieve.join,
            join_level: retrieve.join_level,
            contained_by: retrieve.contained_by,
            ..TemporalQueryParameters::default()
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::ParameterError,
        query::temporal::{TemporalQueryParameters, operation::TemporalQueryOperation, retrieve_parameters::TemporalRetrieveParameters},
    };
    use andromeda_linked_retrieval::admission::TemporalJoin;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn params(query: &str) -> TemporalRetrieveParameters {
        serde_urlencoded::from_str(query).expect("a decodable retrieval")
    }

    /// Table 6.19.3.1-1, with `format`, `options`, `local` and the clause
    /// 5.7.3.3 Entity Map members, reaches the temporal query model.
    #[test]
    fn the_retrieval_parameters_reach_the_query_model() {
        let query = TemporalQueryParameters::from(params(
            "timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-02-01T00:00:00Z&timeproperty=modifiedAt&lastN=3&attrs=speed&datasetId=@none&lang=en&format=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=PT1H&local=true&entityMap=true&entityMapLifetime=PT1H",
        ));
        assert!(query.timerel.is_some() && query.time_at.is_some() && query.end_time_at.is_some() && query.timeproperty.is_some());
        assert_eq!(query.last_n.map(NonZeroU32::get), Some(3));
        assert!(query.attrs.is_some() && query.dataset_id.is_some() && query.lang.is_some() && query.format.is_some());
        assert!(query.aggr_methods.is_some() && query.aggr_period_duration.is_some());
        assert_eq!((query.local, query.entity_map), (Some(true), Some(true)));
        assert!(query.entity_map_lifetime.is_some());
        assert!(query.entity_type.is_none() && query.q.is_none() && query.page.is_absent());
    }

    /// The origin a continuation of a calendar grid keeps reaches the query
    /// model beside the period it lays.
    #[test]
    fn the_period_origin_reaches_the_query_model() {
        let query = TemporalQueryParameters::from(params(
            "timerel=after&timeAt=2020-02-29T00:00:00Z&format=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=P1M&aggrPeriodOrigin=2020-01-31T00:00:00Z",
        ));
        let origin = "2020-01-31T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get();
        assert_eq!(query.aggr_period_origin, Some(origin));
        assert!(query.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Spec).is_ok());
    }

    /// Clause 5.7.3 and Tables 6.19.3.1-1: a winning `temporalValues` format
    /// ignores the aggregation parameters `options=aggregatedValues` admits,
    /// and without `aggregatedValues` anywhere they are refused.
    #[test]
    fn a_retrieval_applies_aggregation_when_format_or_options_names_aggregated_values() {
        let admitted = TemporalQueryParameters::from(params(
            "timerel=after&timeAt=2024-01-01T00:00:00Z&format=temporalValues&options=aggregatedValues&aggrMethods=sum",
        ));
        assert!(admitted.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Spec).is_ok());
        let refused = TemporalQueryParameters::from(params(
            "timerel=after&timeAt=2024-01-01T00:00:00Z&format=temporalValues&aggrMethods=sum&aggrPeriodDuration=PT1H",
        ));
        let error = refused
            .validate(TemporalQueryOperation::Retrieve, TemporalJoin::Spec)
            .expect_err("no aggregated values named");
        assert!(matches!(error, ParameterError::AggregationWithoutAggregatedValues), "{error:?}");
    }

    /// An origin lays periods, so without a non-zero period there is nothing
    /// for it to lay (clause 6.3.20).
    #[test]
    fn a_period_origin_without_a_period_is_invalid() {
        for aggregation in ["", "&aggrPeriodDuration=PT0S"] {
            let query = TemporalQueryParameters::from(params(&format!(
                "format=aggregatedValues&aggrMethods=sum&aggrPeriodOrigin=2020-01-31T00:00:00Z{aggregation}"
            )));
            let error = query.validate(TemporalQueryOperation::Retrieve, TemporalJoin::Spec).expect_err(aggregation);
            assert!(matches!(error, ParameterError::AggrPeriodOriginWithoutPeriods), "{aggregation}: {error:?}");
        }
    }

    /// The extension members are read here and refused by the binding's gate
    /// while their extension is off.
    #[test]
    fn the_extension_members_are_read() {
        let query = TemporalQueryParameters::from(params(
            "join=flat&joinLevel=2&containedBy=urn:ngsi-ld:T:1&pick=speed&omit=type&aggrGroupBy=unitCode",
        ));
        assert!(query.join.is_some() && query.join_level.is_some() && query.contained_by.is_some());
        assert!(query.pick.is_some() && query.omit.is_some() && query.aggr_group_by.is_some());
    }

    /// Table 6.19.3.1-1 selects no entity and clause 4.12 pages no single
    /// history, so each of these is a parameter the operation does not define
    /// (clause 6.3.20).
    #[test]
    fn every_query_only_parameter_is_unknown() {
        for parameter in [
            "type=Vehicle",
            "id=urn:ngsi-ld:Vehicle:1",
            "idPattern=urn:.*",
            "q=speed>1",
            "georel=near%3BmaxDistance==1&geometry=Point&coordinates=[1,2]",
            "geoproperty=location",
            "scopeQ=/Madrid",
            "csf=tier==1",
            "orderBy=id",
            "collation=en",
            "expandValues=speed",
            "jsonKeys=speed",
            "splitEntities=true",
            "count=true",
            "limit=3",
            "offset=1",
            "cursor=abc",
        ] {
            let message = serde_urlencoded::from_str::<TemporalRetrieveParameters>(parameter)
                .expect_err(parameter)
                .to_string();
            assert!(message.contains("unknown field"), "{parameter}: {message}");
        }
    }
}
