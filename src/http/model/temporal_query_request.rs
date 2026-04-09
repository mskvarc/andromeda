//! POST query request body types for the NGSI-LD Temporal query API.

use crate::http::model::query_request::{EntitySelection, GeoQueryRequest};
use serde::Deserialize;

/// POST temporal query request body (NGSI-LD 6.4.3.2 temporal variant).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalQueryRequest {
    #[serde(rename = "type")]
    pub _type: String,
    pub entities: Option<Vec<EntitySelection>>,
    pub attrs: Option<Vec<String>>,
    pub q: Option<String>,
    pub geo_q: Option<GeoQueryRequest>,
    pub temporal_q: Option<TemporalQ>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
    pub format: Option<String>,
    pub last_n: Option<u32>,
    pub local: Option<bool>,
}

/// Temporal filter within a POST temporal query body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalQ {
    pub timerel: String,
    pub time_at: String,
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,
    pub aggr_methods: Option<String>,
    pub aggr_period_duration: Option<String>,
}
