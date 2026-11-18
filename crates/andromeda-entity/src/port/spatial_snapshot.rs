//! Typed reads of the store's entity locations, used to build and catch up an
//! entity spatial index.

use crate::error::EntityError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use futures::future::BoxFuture;
use std::{num::NonZeroUsize, sync::Arc};

/// A cut through the store's commit history, taken by one read. Every change
/// below it is reflected in that read, and none above it is.
///
/// Entity policy never reads it. One read produces it and the next is handed
/// it back, and only the store that produced it can say what its positions
/// count.
/// A change is above the cut when it was made at or above the ceiling, or by
/// one of the unseen positions below it, which had not committed when the read
/// was taken. A read of the changes above the cut therefore misses nothing the
/// read that took it did not already hold, and reads nothing it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeHorizon {
    /// Every change made at or above this position is above the cut.
    ceiling: u64,
    /// The positions below the ceiling whose changes the read could not see.
    unseen: Box<[u64]>,
}

impl ChangeHorizon {
    /// A cut the store reported as `ceiling` with `unseen` below it.
    #[must_use]
    pub fn new(ceiling: u64, unseen: Vec<u64>) -> ChangeHorizon {
        ChangeHorizon {
            ceiling,
            unseen: unseen.into_boxed_slice(),
        }
    }

    /// The store's spelling of the position every later change is made at or
    /// above.
    #[must_use]
    pub const fn ceiling(&self) -> u64 {
        self.ceiling
    }

    /// The store's spelling of the positions below the ceiling whose changes
    /// the read could not see.
    #[must_use]
    pub const fn unseen(&self) -> &[u64] {
        &self.unseen
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

/// The entity locations one change read returned, and the horizon it was read
/// at.
#[derive(Debug)]
pub struct LocationRead {
    /// The entity locations the read returned.
    pub locations: Vec<EntityLocation>,
    /// The horizon the read's snapshot was taken at.
    pub horizon: ChangeHorizon,
}

/// What a change read bounded to a number of entities found.
#[derive(Debug)]
pub enum BoundedChangeRead {
    /// No more entities changed than the bound, so the read returns every one
    /// of them and its horizon.
    Within(LocationRead),
    /// More entities changed than the bound. None of them is returned, and the
    /// read moves no horizon.
    Exceeded,
}

/// Takes in the located entities of a read of every location, a batch at a
/// time, as the store decodes them.
pub trait LocationSink: Send + Sync {
    /// Take in one batch of located entities.
    ///
    /// Taking a batch in costs work in proportion to its size, so a source
    /// hands each batch in where it decoded it, off the threads requests run
    /// on.
    fn take(&self, locations: Vec<EntityLocation>);
}

/// Reads entity locations without mutating an in-memory index.
pub trait EntitySpatialSnapshotSource: Send + Sync + 'static {
    /// Read every live entity that conveys a default `location`, handing them
    /// to `sink` in batches as the store returns them, and return the horizon
    /// the read was taken at.
    ///
    /// The read takes as long as the store is large, and no request waits for
    /// it. A batch is handed in as soon as it is decoded, so the read never
    /// holds every entity at once. A read that fails part way may have handed
    /// `sink` some of them.
    fn locations<'life0, 'async_trait>(&'life0 self, sink: Arc<dyn LocationSink>) -> BoxFuture<'async_trait, Result<ChangeHorizon, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Read every live entity whose types or geometries changed above
    /// `horizon`, with or without a `location` now, together with the horizon
    /// this read was taken at.
    ///
    /// The read takes as long as the changes are many, and no request waits for
    /// it.
    fn locations_changed_since<'life0, 'life1, 'async_trait>(
        &'life0 self,
        horizon: &'life1 ChangeHorizon,
    ) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Read what [`EntitySpatialSnapshotSource::locations_changed_since`]
    /// reads when at most `bound` entities changed above `horizon`, and only
    /// learn that more did otherwise.
    ///
    /// Bounded by `bound` entities, so a request may wait for it.
    fn locations_changed_within<'life0, 'life1, 'async_trait>(
        &'life0 self,
        horizon: &'life1 ChangeHorizon,
        bound: NonZeroUsize,
    ) -> BoxFuture<'async_trait, Result<BoundedChangeRead, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
