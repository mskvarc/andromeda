use crate::domain::{attribute::AttributeValue, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// NGSI-LD ListProperty (spec 5.2.36)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProperty {
    #[serde(rename = "valueList")]
    pub value_list: Vec<Value>,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,

    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
