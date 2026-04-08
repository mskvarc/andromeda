use crate::domain::{attribute::AttributeValue, uri::Uri};
use chrono::{DateTime, Utc};
use geojson::Geometry;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD GeoProperty representation (ETSI GS CIM 009 clause 5.2.7).
///
/// A GeoProperty represents an attribute whose value is a GeoJSON geometry.
/// Contains the geometry value, optional metadata (observedAt, datasetId),
/// and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoProperty {
    pub value: Geometry,
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
