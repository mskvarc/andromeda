use crate::domain::{attribute::AttributeValue, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// NGSI-LD ListRelationship representation (ETSI GS CIM 009 clause 5.2.37).
///
/// A ListRelationship represents an attribute whose value is an ordered list
/// of entity references (URIs). Contains the object list, optional metadata
/// (observedAt, datasetId), and optional sub-attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRelationship {
    pub object_list: Vec<Uri>,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
