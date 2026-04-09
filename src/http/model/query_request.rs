//! POST query request body types for the NGSI-LD Entity query API.

use serde::Deserialize;
use serde_json::Value;

/// POST query request body (NGSI-LD 6.4.3.2).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// Deserialized from the request body for serde acceptance but not used
    /// directly -- entity type filtering is handled via `entities[].type`.
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub type_: Option<String>,
    pub entities: Option<Vec<EntitySelection>>,
    pub attrs: Option<Vec<String>>,
    pub q: Option<String>,
    pub geo_q: Option<GeoQueryRequest>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
}

/// Entity selector within a POST query body.
///
/// Supports both the full object form (`{"id": "...", "type": "..."}`) and the
/// short string form (just an entity ID).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EntitySelection {
    /// Full object form: `{"id": "...", "type": "...", "idPattern": "..."}`.
    Object {
        id: Option<String>,
        #[serde(rename = "idPattern")]
        id_pattern: Option<String>,
        #[serde(rename = "type")]
        type_: Option<String>,
    },
    /// Short form: just an entity ID string.
    Id(String),
}

impl EntitySelection {
    /// Returns the entity ID, if present.
    pub fn id(&self) -> Option<&str> {
        match self {
            EntitySelection::Object { id, .. } => id.as_deref(),
            EntitySelection::Id(s) => Some(s.as_str()),
        }
    }

    /// Returns the ID pattern, if present.
    pub fn id_pattern(&self) -> Option<&str> {
        match self {
            EntitySelection::Object { id_pattern, .. } => id_pattern.as_deref(),
            EntitySelection::Id(_) => None,
        }
    }

    /// Returns the entity type, if present.
    pub fn type_(&self) -> Option<&str> {
        match self {
            EntitySelection::Object { type_, .. } => type_.as_deref(),
            EntitySelection::Id(_) => None,
        }
    }
}

/// Geo-query filter within a POST query body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoQueryRequest {
    pub georel: String,
    pub geometry: String,
    pub coordinates: Value,
    pub geoproperty: Option<String>,
}
