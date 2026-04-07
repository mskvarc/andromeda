//! In-memory store for NGSI-LD entity maps (ETSI GS CIM 009 clause 5.2.39).
//!
//! Backed by a concurrent `DashMap` with automatic expiry cleanup on insert.

use crate::domain::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;

/// In-memory store for entity maps with automatic expiry cleanup.
#[derive(Debug)]
pub struct EntityMapStore {
    maps: DashMap<String, EntityMap>,
}

impl EntityMapStore {
    /// Creates an empty entity map store.
    pub fn new() -> EntityMapStore {
        EntityMapStore { maps: DashMap::new() }
    }

    /// Inserts an entity map, removing any expired entries opportunistically.
    pub async fn insert(&self, map: EntityMap) {
        // Opportunistic cleanup of expired maps
        self.maps.retain(|_, m| !m.is_expired());
        self.maps.insert(map.id.clone(), map); // clone: owned key for map entry (map moves into value)
    }

    /// Retrieves an entity map by ID, returning `None` if not found or expired.
    pub async fn get(&self, id: &str) -> Option<EntityMap> {
        self.maps.get(id).filter(|m| !m.is_expired()).map(|r| r.clone()) // clone: owned copy to return from DashMap ref
    }

    /// Updates the expiry time of an entity map. Returns `false` if not found or already expired.
    pub async fn update(&self, id: &str, expires_at: DateTime<Utc>) -> bool {
        if let Some(mut entry) = self.maps.get_mut(id) {
            if entry.is_expired() {
                drop(entry);
                self.maps.remove(id);
                return false;
            }
            entry.expires_at = expires_at;
            true
        } else {
            false
        }
    }

    /// Deletes an entity map by ID. Returns `true` if the entry existed.
    pub async fn delete(&self, id: &str) -> bool {
        self.maps.remove(id).is_some()
    }
}
