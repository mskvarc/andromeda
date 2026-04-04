use crate::domain::{attribute::AttributeValue, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use geojson::Geometry;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// NGSI-LD GeoProperty representation (ETSI GS CIM 009 clause 5.2.7).
///
/// A GeoProperty represents an attribute whose value is a GeoJSON geometry.
/// Contains the geometry value, optional metadata (observedAt, datasetId),
/// and optional sub-attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoProperty {
    pub value: Geometry,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
