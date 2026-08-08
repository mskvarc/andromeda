//! In-memory store for NGSI-LD entity maps (ETSI GS CIM 009 clause 5.2.39).
//!
//! Backed by a concurrent `DashMap` with opportunistic expiry cleanup on insert.
//! The `EntityMapStore` trait impl lives in the binary crate, because the trait
//! is defined in the binary's service layer.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::EntityMapId;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use dashmap::DashMap;

/// In-memory entity map store with opportunistic expiry cleanup.
#[derive(Debug, Default)]
pub struct InMemoryEntityMapStore {
    /// The concurrent map from entity map identifier to stored map.
    maps: DashMap<EntityMapId, EntityMap>,
}

impl InMemoryEntityMapStore {
    /// Creates an empty entity map store.
    pub fn new() -> InMemoryEntityMapStore {
        InMemoryEntityMapStore { maps: DashMap::new() }
    }

    /// Returns a reference to the underlying map, for trait impls defined
    /// outside this crate.
    pub const fn maps(&self) -> &DashMap<EntityMapId, EntityMap> {
        &self.maps
    }
}
