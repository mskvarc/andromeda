use crate::{
    error::Result,
    representation::{NgsiLdRepresentation, NgsiLdSkipNull, ToRepresentation, concise, normalized, simplified},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use urn::Urn;

/// NGSI-LD ListRelationship (clause 4.5.17).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ListRelationship {
    #[serde(rename = "objectList")]
    pub object_list: Vec<Urn>,
    #[serde(rename = "objectType", skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
}

impl ListRelationship {
    pub fn new(object_list: Vec<Urn>) -> Self {
        Self {
            object_list,
            object_type: None,
            observed_at: None,
        }
    }
}

impl ToRepresentation for ListRelationship {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_list_relationship(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_list_relationship(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_list_relationship(self, skip_null)?)),
        }
    }
}
