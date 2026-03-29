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

/// NGSI-LD Property (clause 4.5.2).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Property {
    pub value: Value,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "unitCode", skip_serializing_if = "Option::is_none")]
    pub unit_code: Option<String>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: IndexMap<NameBuf, Box<AttributeWrapper>>,
}

impl Property {
    pub fn new(value: impl Into<Value>) -> Self {
        Self {
            value: value.into(),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::new(),
        }
    }
}

impl ToRepresentation for Property {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_property(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_property(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_property(self, skip_null)?)),
        }
    }
}
