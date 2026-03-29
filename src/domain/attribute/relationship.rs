use crate::domain::attribute::AttributeValue;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use ngsi_ld_types::{Uri, time_format};
use serde::{Deserialize, Serialize};

/// NGSI-LD Relationship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub object: Uri,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none", with = "time_format::rfc3339_option", default)]
    pub observed_at: Option<DateTime<Utc>>,

    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}
