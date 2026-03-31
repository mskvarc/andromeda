use crate::domain::{attribute::AttributeValue, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// NGSI-LD VocabProperty representation (ETSI GS CIM 009 clause 5.2.35).
///
/// A VocabProperty represents an attribute whose value is a vocabulary term
/// (compact IRI or full IRI). Contains the vocab value, optional previous
/// value, metadata (observedAt, datasetId), and optional sub-attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabProperty {
    pub vocab: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_vocab: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
