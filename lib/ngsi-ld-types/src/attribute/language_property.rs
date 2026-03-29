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

/// NGSI-LD LanguageProperty (clause 4.5.9).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LanguageProperty {
    #[serde(rename = "languageMap")]
    pub language_map: IndexMap<String, String>,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: IndexMap<NameBuf, Box<AttributeWrapper>>,
}

impl LanguageProperty {
    pub fn new(language_map: IndexMap<String, String>) -> Self {
        Self {
            language_map,
            observed_at: None,
            dataset_id: None,
            attributes: IndexMap::new(),
        }
    }
}

impl ToRepresentation for LanguageProperty {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_language_property(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_language_property(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_language_property(self, skip_null)?)),
        }
    }
}
