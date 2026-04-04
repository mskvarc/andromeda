use crate::domain::{attribute::AttributeValue, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// NGSI-LD ListProperty representation (ETSI GS CIM 009 clause 5.2.36).
///
/// A ListProperty represents an attribute whose value is an ordered list
/// of JSON values. Contains the value list, optional metadata
/// (observedAt, datasetId), and optional sub-attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProperty {
    pub value_list: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
