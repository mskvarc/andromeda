use crate::{
    attribute::AttributeWrapper,
    error::Result,
    name::NameBuf,
    representation::{NgsiLdRepresentation, NgsiLdSkipNull, ToRepresentation, concise, normalized, simplified},
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use urn::Urn;

/// NGSI-LD Relationship (clause 4.5.5).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Relationship {
    pub object: Urn,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: IndexMap<NameBuf, Box<AttributeWrapper>>,
}

impl Relationship {
    pub fn new(object: Urn) -> Self {
        Self {
            object,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::new(),
        }
    }
}

impl ToRepresentation for Relationship {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_relationship(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_relationship(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_relationship(self, skip_null)?)),
        }
    }
}
