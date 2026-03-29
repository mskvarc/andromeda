use crate::domain::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tokio::sync::RwLock;

pub struct EntityMapStore {
    maps: RwLock<HashMap<String, EntityMap>>,
}

impl EntityMapStore {
    pub fn new() -> Self {
        Self {
            maps: RwLock::new(HashMap::new()),
        }
    }

    pub async fn insert(&self, map: EntityMap) {
        let mut store = self.maps.write().await;
        // Opportunistic cleanup of expired maps
        store.retain(|_, m| !m.is_expired());
        store.insert(map.id.clone(), map);
    }

    pub async fn get(&self, id: &str) -> Option<EntityMap> {
        let store = self.maps.read().await;
        store.get(id).filter(|m| !m.is_expired()).cloned()
    }

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

    pub async fn delete(&self, id: &str) -> bool {
        let mut store = self.maps.write().await;
        store.remove(id).is_some()
    }
}
