use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::Serialize;

/// NGSI-LD EntityMap representation (ETSI GS CIM 009 clause 5.2.39).
///
/// Maps entity identifiers to their dataset IDs for distributed query results.
/// Used by the broker to track which entities were retrieved from which
/// context sources and to support linked entity map navigation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityMap {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: &'static str,
    pub expires_at: DateTime<Utc>,
    #[serde(skip)]
    pub entity_ids: Vec<String>,
    pub entity_map: IndexMap<String, Vec<String>>,
    pub linked_maps: IndexMap<String, String>,
}

impl EntityMap {
    pub fn new(id: String, expires_at: DateTime<Utc>, entity_ids: Vec<String>) -> Self {
        let mut entity_map = IndexMap::new();
        for eid in &entity_ids {
            entity_map.insert(eid.clone(), vec!["@none".to_string()]);
        }
        Self {
            id,
            type_: "EntityMap",
            expires_at,
            entity_ids,
            entity_map,
            linked_maps: IndexMap::new(),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}
