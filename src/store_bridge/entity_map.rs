//! `EntityMapStore` trait implementation for `InMemoryEntityMapStore`.

use crate::service::entity_map::store::EntityMapStore;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::EntityMapId;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_postgresql_context_store::entity_map::InMemoryEntityMapStore;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl EntityMapStore for InMemoryEntityMapStore {
    async fn insert(&self, map: EntityMap) {
        self.maps().retain(|_, m| !m.is_expired());
        self.maps().insert(map.id.clone(), map); // clone: owned key for the entry, since the map moves into the value
    }

    async fn get(&self, id: &EntityMapId) -> Option<EntityMap> {
        self.maps().get(id).filter(|m| !m.is_expired()).map(|r| r.clone()) // clone: owned copy to return from the DashMap ref
    }

    async fn update(&self, id: &EntityMapId, expires_at: DateTime<Utc>) -> bool {
        if let Some(mut entry) = self.maps().get_mut(id) {
            if entry.is_expired() {
                drop(entry);
                self.maps().remove(id);
                return false;
            }
            entry.expires_at = expires_at;
            true
        } else {
            false
        }
    }

    async fn delete(&self, id: &EntityMapId) -> bool {
        self.maps().remove(id).is_some()
    }
}
