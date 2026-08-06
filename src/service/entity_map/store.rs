//! The entity map store port (clause 5.2.39).
//!
//! Entity maps are bounded, short-lived and expire on their own schedule, so
//! nothing in the contract commits an implementation to durability -- an
//! in-process map and a shared cache both satisfy it.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::EntityMapId;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Storing entity maps and letting them expire.
#[async_trait]
pub trait EntityMapStore: Send + Sync + 'static {
    /// Store an entity map.
    async fn insert(&self, map: EntityMap);

    /// Retrieve an entity map, treating an expired one as absent.
    async fn get(&self, id: &EntityMapId) -> Option<EntityMap>;

    /// Move an entity map's expiry. Returns `false` if it is absent or already expired.
    async fn update(&self, id: &EntityMapId, expires_at: DateTime<Utc>) -> bool;

    /// Remove an entity map. Returns whether it existed.
    async fn delete(&self, id: &EntityMapId) -> bool;
}
