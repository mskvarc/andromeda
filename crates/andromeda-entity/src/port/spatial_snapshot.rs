//! Typed snapshots used to rebuild an entity spatial index.

use crate::error::EntityError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use futures::future::BoxFuture;

/// One entity location and the types whose spatial partitions contain it.
#[derive(Debug)]
pub struct EntitySpatialSnapshot {
    /// Entity identifier.
    pub entity_id: Uri,
    /// Entity types associated with this geometry.
    pub entity_types: Vec<EntityTypeName>,
    /// Validated location geometry.
    pub geometry: Geometry,
}

/// Reads typed spatial snapshots without mutating an in-memory index.
pub trait EntitySpatialSnapshotSource: Send + Sync + 'static {
    /// Read every entity with a valid default `location` geoproperty.
    fn location_snapshots<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<EntitySpatialSnapshot>, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
