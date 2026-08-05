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
    /// Entity selectors constraining the query.
    pub entities: Option<Vec<EntitySelection>>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    pub attrs: Option<Vec<String>>,
    /// The `q` filter expression (clause 4.9), in compacted form.
    pub q: Option<String>,
    /// The geo-query, as the `georel`/`geometry`/`coordinates` triple.
    pub geo_q: Option<GeoQueryRequest>,
    /// The page size (clause 5.5.9 `limit`).
    pub limit: Option<u32>,
    /// How many results to skip before the page (`offset`).
    pub offset: Option<u32>,
    /// Whether the response reports the total number of matches (`count`).
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
        /// Entity identifiers to select, comma-separated (clause 6.3.x `id`).
        id: Option<String>,
        /// A regular expression the selected identifiers must match (`idPattern`).
        #[serde(rename = "idPattern")]
        id_pattern: Option<String>,
        /// The entity type to select.
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
    /// The spatial relationship of the geo-query (clause 4.10 `georel`).
    pub georel: String,
    /// The GeoJSON geometry type of the geo-query's reference (`geometry`).
    pub geometry: String,
    /// The coordinates of the geo-query's reference geometry (`coordinates`).
    pub coordinates: Value,
    /// The GeoProperty the geo-query applies to (`geoproperty`).
    pub geoproperty: Option<String>,
}
