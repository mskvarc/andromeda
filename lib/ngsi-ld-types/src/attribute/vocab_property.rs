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

/// NGSI-LD VocabProperty (clause 4.5.11).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct VocabProperty {
    #[serde(rename = "hasVocab")]
    pub has_vocab: String,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: IndexMap<NameBuf, Box<AttributeWrapper>>,
}

impl VocabProperty {
    pub fn new(has_vocab: impl Into<String>) -> Self {
        Self {
            has_vocab: has_vocab.into(),
            observed_at: None,
            dataset_id: None,
            attributes: IndexMap::new(),
        }
    }
}

impl ToRepresentation for VocabProperty {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_vocab_property(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_vocab_property(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_vocab_property(self, skip_null)?)),
        }
    }
}
