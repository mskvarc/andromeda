use crate::domain::{attribute::AttributeValue, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use geojson::Geometry;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// NGSI-LD GeoProperty
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoProperty {
    pub value: Geometry,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
