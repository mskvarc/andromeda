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
    /// Store an Entity Map with its entries, links and creating-request
    /// fingerprint, replacing a map with the same identifier.
    fn insert<'a>(&'a self, map: &'a EntityMap) -> EntityMapStoreFuture<'a, ()>;

    /// Retrieve a live Entity Map, treating an expired map as absent.
    fn get<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, Option<EntityMap>>;

    /// Move a live Entity Map's expiry.
    fn update<'a>(&'a self, identifier: &'a Uri, expires_at: DateTime<Utc>) -> EntityMapStoreFuture<'a, EntityMapMutation>;

    /// Record the map a registration's context source used under a live map
    /// (clause 5.5.14), replacing the link recorded for that registration.
    fn link<'a>(&'a self, identifier: &'a Uri, registration: &'a Uri, linked_map: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation>;

    /// Remove `entity_ids`, with every source attributed to them, from a live
    /// Entity Map.
    ///
    /// Clause 4.3.6.7 sheds "identifiers of Entities not matching the filters at
    /// the time of checking" from the map while a result page is prepared, and
    /// clause 5.5.14 makes that the duty of the broker that created it. An
    /// identifier the map does not list, and a map that expired meanwhile, are
    /// both left exactly as they are.
    fn prune<'a>(&'a self, identifier: &'a Uri, entity_ids: &'a [Uri]) -> EntityMapStoreFuture<'a, ()>;

    /// Remove a live Entity Map.
    fn delete<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation>;

    /// Remove every Entity Map whose expiry is at or before `cutoff`, returning how many.
    fn delete_expired(&self, cutoff: DateTime<Utc>) -> EntityMapStoreFuture<'_, u64>;
}

#[cfg(test)]
mod tests {
    use crate::store::EntityMapMutation;

    #[test]
    fn mutation_outcomes_distinguish_applied_from_missing() {
        assert_ne!(EntityMapMutation::Applied, EntityMapMutation::Missing);
    }
}
