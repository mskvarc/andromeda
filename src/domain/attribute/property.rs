use crate::domain::{attribute::AttributeValue, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iref::IriBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD Property representation (ETSI GS CIM 009 clause 5.2.5).
///
/// A Property represents an attribute whose value is any JSON value.
/// Contains the value, optional metadata (observedAt, unitCode, datasetId),
/// and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub value: Value,
    pub unit_code: Option<String>,
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<IriBuf, AttributeValue>,
}
