//! Typed reads of the store's entity locations, used to build and catch up an
//! entity spatial index.

use crate::error::EntityError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use futures::future::BoxFuture;

/// A position in the store's commit history below which every change is
/// already reflected in a read.
///
/// Opaque to entity policy: it is produced by one read and handed back to the
/// next, and only the store that produced it can say what it counts. Every
/// change that committed before the read's snapshot and is not already
/// reflected by it is at or above the horizon, so a read of the changes since
/// the horizon misses nothing the previous read did not already hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChangeHorizon(u64);

impl ChangeHorizon {
    /// A horizon the store reported as `position`.
    #[must_use]
    pub const fn new(position: u64) -> ChangeHorizon {
        ChangeHorizon(position)
    }

    /// The store's own spelling of this horizon.
    #[must_use]
    pub const fn position(self) -> u64 {
        self.0
    }
}

/// One entity's default `location` and the types whose partitions hold it.
#[derive(Debug)]
pub struct EntityLocation {
    /// Entity identifier.
    pub entity_id: Identifier,
    /// Entity types the entity carries.
    pub entity_types: Vec<EntityTypeName>,
    /// The geometry of the entity's default `location` instance, exactly as the
    /// store evaluates a geo query against it, or `None` when the entity
    /// conveys none.
    pub location: Option<Geometry>,
}

/// The entity locations one read returned, and the horizon it was read at.
#[derive(Debug)]
pub struct LocationRead {
    /// The entity locations the read returned.
    pub locations: Vec<EntityLocation>,
    /// The horizon the read's snapshot was taken at.
    pub horizon: ChangeHorizon,
}

/// Reads entity locations without mutating an in-memory index.
pub trait EntitySpatialSnapshotSource: Send + Sync + 'static {
    /// Read every live entity that conveys a default `location`, together with
    /// the horizon the read was taken at.
    fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Read every live entity whose types or geometries changed at or above
    /// `horizon` -- with or without a `location` now -- together with the
    /// horizon this read was taken at.
    fn locations_changed_since<'life0, 'async_trait>(&'life0 self, horizon: ChangeHorizon) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
