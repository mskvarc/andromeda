//! The entity map store port (clause 5.2.39).
//!
//! Entity maps are bounded, short-lived and expire on their own schedule, so
//! nothing in the contract commits an implementation to durability -- an
//! in-process map and a shared cache both satisfy it.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;

/// Storing entity maps and letting them expire.
pub trait EntityMapStore: Send + Sync + 'static {
    /// Store an entity map.
    fn insert<'life0, 'async_trait>(&'life0 self, map: EntityMap) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Retrieve an entity map, treating an expired one as absent.
    fn get<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Option<EntityMap>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Move an entity map's expiry. Returns `false` if it is absent or already expired.
    fn update<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri, expires_at: DateTime<Utc>) -> BoxFuture<'async_trait, bool>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Remove an entity map. Returns whether it existed.
    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, bool>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
