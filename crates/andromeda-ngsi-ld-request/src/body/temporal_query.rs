//! POST query request body types for the NGSI-LD Temporal query API.

use crate::body::query::{EntitySelection, GeoQueryRequest};
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
    pub attrs: Option<Vec<String>>,
    /// The `q` filter expression (clause 4.9), in compacted form.
    pub q: Option<String>,
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
    pub format: Option<String>,
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
    pub timerel: String,
    /// The instant the window is anchored at (`timeAt`).
    pub time_at: String,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    pub end_time_at: Option<String>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    pub timeproperty: Option<String>,
    /// The aggregation methods, comma-separated (clause 4.5.19 `aggrMethods`).
    pub aggr_methods: Option<String>,
    /// The aggregation bucket width, as an ISO 8601 duration (`aggrPeriodDuration`).
    pub aggr_period_duration: Option<String>,
}
