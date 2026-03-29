use crate::{
    error::Result,
    representation::{NgsiLdRepresentation, NgsiLdSkipNull, ToRepresentation, concise, normalized, simplified},
};
use chrono::{DateTime, Utc};
use ngsi_ld_value::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// NGSI-LD GeoProperty (clause 4.5.7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoProperty {
    pub value: Value,
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

impl GeoProperty {
    pub fn new(value: impl Into<Value>) -> Self {
        Self {
            value: value.into(),
            observed_at: None,
            dataset_id: None,
            instance_id: None,
        }
    }
}

impl ToRepresentation for GeoProperty {
    fn to_representation(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<Option<JsonValue>> {
        match representation {
            NgsiLdRepresentation::Normalized => Ok(Some(normalized::serialize_geo_property(self, skip_null)?)),
            NgsiLdRepresentation::Concise => Ok(Some(concise::serialize_geo_property(self, skip_null)?)),
            NgsiLdRepresentation::Simplified => Ok(Some(simplified::serialize_geo_property(self, skip_null)?)),
        }
    }
}
