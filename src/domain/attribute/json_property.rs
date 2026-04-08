use crate::domain::{attribute::AttributeValue, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD JsonProperty representation (ETSI GS CIM 009 clause 5.2.38).
///
/// A JsonProperty represents an attribute whose value is an arbitrary JSON
/// document. Contains the JSON value, optional metadata (observedAt, datasetId),
/// and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonProperty {
    pub json: Value,
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
