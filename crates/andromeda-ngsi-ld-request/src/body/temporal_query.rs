//! POST query request body types for the NGSI-LD Temporal query API.

use crate::{
    body::query::{EntitySelection, GeoQueryRequest},
    query::deserialization::deserialize_optional_filter_expr,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    temporal::aggregation_period::{AggregationPeriod, optional},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// POST temporal query request body (NGSI-LD 6.4.3.2 temporal variant).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalQueryRequest {
    /// The resource type of the request body, accepted for the spec's shape and not read.
    #[serde(rename = "type")]
    pub _type: String,
    /// Entity selectors constraining the query.
    pub entities: Option<Vec<EntitySelection>>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    pub attrs: Option<Vec<AttributeName>>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// The geo-query, as the `georel`/`geometry`/`coordinates` triple.
    pub geo_q: Option<GeoQueryRequest>,
    /// The temporal window the query applies to.
    pub temporal_q: Option<TemporalQ>,
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Whether the response reports the total number of matches (`count`).
    pub count: Option<bool>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialize_option_temporal_format")]
    pub format: Option<TemporalFormat>,
    /// How many instances per attribute the response carries (`lastN`).
    pub last_n: Option<u32>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    pub local: Option<bool>,
}

/// Temporal filter within a POST temporal query body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalQ {
    /// How the window relates to the given instants (clause 4.11 `timerel`).
    #[serde(deserialize_with = "crate::query::temporal::deserialize_timerel")]
    pub timerel: TimeRel,
    /// The instant the window is anchored at (`timeAt`).
    pub time_at: DateTime<Utc>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialize_option_timeproperty")]
    pub timeproperty: Option<TimeProp>,
    /// The aggregation methods, comma-separated (clause 4.5.19 `aggrMethods`).
    #[serde(default, deserialize_with = "crate::query::temporal::deserialize_optional_aggr_methods")]
    pub aggr_methods: Option<Vec<andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod>>,
    /// The aggregation bucket width, as an ISO 8601 duration (`aggrPeriodDuration`).
    #[serde(default, with = "optional")]
    pub aggr_period_duration: Option<AggregationPeriod>,
}
