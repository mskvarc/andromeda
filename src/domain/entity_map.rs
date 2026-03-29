use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EntityMap {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: &'static str,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
    #[serde(skip)]
    pub entity_ids: Vec<String>,
    #[serde(rename = "entityMap")]
    pub entity_map: IndexMap<String, Vec<String>>,
    #[serde(rename = "linkedMaps")]
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
