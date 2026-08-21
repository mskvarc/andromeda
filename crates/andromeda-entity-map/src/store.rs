//! The replaceable storage capability for Entity Maps.
//!
//! An Entity Map is shared state of a distributed query (clause 5.5.14): a
//! follow-up page may reach any instance serving the same tenant, so the store
//! is durable and tenant-scoped rather than a per-process cache.

use crate::error::EntityMapError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use std::{future::Future, pin::Pin};

/// Owned future returned by an object-safe Entity Map store implementation.
pub type EntityMapStoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, EntityMapError>> + Send + 'a>>;

/// Whether an Entity Map mutation found a live stored map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMapMutation {
    /// The mutation was applied to a live map.
    Applied,
    /// The map was absent or had expired.
    Missing,
}

/// Stores Entity Maps, with expiry decided by the store's own clock so every
/// instance sharing the store agrees on what is live.
pub trait EntityMapStore: Send + Sync + 'static {
    /// Store an Entity Map with its entries and links, replacing a map with the same identifier.
    fn insert<'a>(&'a self, map: &'a EntityMap) -> EntityMapStoreFuture<'a, ()>;

    /// Retrieve a live Entity Map, treating an expired map as absent.
    fn get<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, Option<EntityMap>>;

    /// Move a live Entity Map's expiry.
    fn update<'a>(&'a self, identifier: &'a Uri, expires_at: DateTime<Utc>) -> EntityMapStoreFuture<'a, EntityMapMutation>;

    /// Remove a live Entity Map.
    fn delete<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation>;

    /// Remove every Entity Map whose expiry is at or before `cutoff`, returning how many.
    fn delete_expired<'a>(&'a self, cutoff: DateTime<Utc>) -> EntityMapStoreFuture<'a, u64>;
}

#[cfg(test)]
mod tests {
    use crate::store::EntityMapMutation;

    #[test]
    fn mutation_outcomes_distinguish_applied_from_missing() {
        assert_ne!(EntityMapMutation::Applied, EntityMapMutation::Missing);
    }
}
