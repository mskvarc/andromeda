use crate::{
    attribute::AttributeWrapper,
    error::Result,
    name::NameBuf,
    representation::{NgsiLdRepresentation, NgsiLdSkipNull, ToRepresentation, concise, normalized, simplified},
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use ngsi_ld_value::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// NGSI-LD ListProperty (clause 4.5.15).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ListProperty {
    #[serde(rename = "hasValueList")]
    pub has_value_list: Vec<Value>,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: IndexMap<NameBuf, Box<AttributeWrapper>>,
}

impl ListProperty {
    pub fn new(has_value_list: Vec<Value>) -> Self {
        Self {
            has_value_list,
            observed_at: None,
            dataset_id: None,
            attributes: IndexMap::new(),
        }
    }
}

impl ToRepresentation for ListProperty {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_list_property(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_list_property(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_list_property(self, skip_null)?)),
        }
    }
}
