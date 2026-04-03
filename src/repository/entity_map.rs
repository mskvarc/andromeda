use crate::domain::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// In-memory store for entity maps with automatic expiry cleanup.
pub struct EntityMapStore {
    maps: RwLock<HashMap<String, EntityMap>>,
}

impl EntityMapStore {
    /// Creates an empty entity map store.
    pub fn new() -> EntityMapStore {
        EntityMapStore {
            maps: RwLock::new(HashMap::new()),
        }
    }

    /// Inserts an entity map, removing any expired entries opportunistically.
    pub async fn insert(&self, map: EntityMap) {
        let mut store = self.maps.write().await;
        // Opportunistic cleanup of expired maps
        store.retain(|_, m| !m.is_expired());
        store.insert(map.id.clone(), map); // clone: owned key for map entry (map moves into value)
    }

    /// Retrieves an entity map by ID, returning `None` if not found or expired.
    pub async fn get(&self, id: &str) -> Option<EntityMap> {
        let store = self.maps.read().await;
        store.get(id).filter(|m| !m.is_expired()).cloned() // clone: owned copy to return from read-locked map
    }

    /// Updates the expiry time of an entity map. Returns `false` if not found or already expired.
    pub async fn update(&self, id: &str, expires_at: DateTime<Utc>) -> bool {
        let mut store = self.maps.write().await;
        if let Some(map) = store.get_mut(id) {
            if map.is_expired() {
                store.remove(id);
                return false;
            }
            map.expires_at = expires_at;
            true
        } else {
            false
        }
    }

    /// Deletes an entity map by ID. Returns `true` if the entry existed.
    pub async fn delete(&self, id: &str) -> bool {
        let mut store = self.maps.write().await;
        store.remove(id).is_some()
    }
}
