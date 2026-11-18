//! Rebuilding the in-memory entity spatial index from a read of the store.
//!
//! A rebuild is two phases. The read holds nothing, so geo queries and every
//! other writer of the index carry on while the store is read. The install
//! groups the read by partition and bulk-loads every partition, which takes
//! seconds for millions of entities, so it runs on a blocking worker rather
//! than on the async runtime.

use crate::{
    error::EntityError,
    port::{
        blocking_executor::{BlockingExecutor, BlockingTask},
        spatial_snapshot::{ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
    },
    spatial_placement::LocationPlacement,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_spatial_index::{
    entity::{entry::SpatialEntry, index::SpatialEntityIndex},
    key::SpatialKey,
};
use std::{collections::HashMap, num::NonZeroUsize, sync::Arc, time::Instant};
use tracing::info;

/// Result of a rebuild pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildOutcome {
    /// Number of located entities the store's read returned.
    pub located_entities: usize,
    /// The horizon the store's read was taken at.
    pub horizon: ChangeHorizon,
}

/// A read of every located entity, and the index generation captured before
/// it was taken.
#[derive(Debug)]
pub struct LocationSnapshot {
    /// The index generation captured before the read. Every write applied to
    /// the index after it keeps its state over the read's.
    generation: u64,
    /// The store's read.
    read: LocationRead,
}

impl LocationSnapshot {
    /// The horizon the store's read was taken at.
    #[must_use]
    pub const fn horizon(&self) -> &ChangeHorizon {
        &self.read.horizon
    }
}

/// Read every located entity of the store for a rebuild of `index`.
///
/// The generation is captured before the read, so a write applied to the
/// index while the store is read keeps its state over the read's.
///
/// # Errors
///
/// Reports [`EntityError::Persistence`] when the store cannot be read.
pub async fn read_location_snapshot(index: &SpatialEntityIndex, source: &dyn EntitySpatialSnapshotSource) -> Result<LocationSnapshot, EntityError> {
    let generation = index.current_generation();
    let read = source.locations().await?;
    Ok(LocationSnapshot { generation, read })
}

/// Rebuild every `location` partition of one tenant's index from `snapshot`,
/// and seed the scope so the index may narrow queries against it.
///
/// `held` is kept until the install has finished, whether or not the caller
/// still waits for it, so a lock it guards cannot be taken while the install
/// still runs.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the blocking worker does not
/// complete the install.
pub async fn install_location_snapshot<Held: Send + 'static>(
    index: &Arc<SpatialEntityIndex>,
    placement: &LocationPlacement,
    snapshot: LocationSnapshot,
    executor: &dyn BlockingExecutor,
    held: Held,
) -> Result<RebuildOutcome, EntityError> {
    let LocationSnapshot { generation, read } = snapshot;
    let outcome = RebuildOutcome {
        located_entities: read.locations.len(),
        horizon: read.horizon,
    };
    let install = Install {
        index: Arc::clone(index),
        placement: placement.clone(), // clone: the blocking worker owns everything it reads
        locations: read.locations,
        generation,
        held,
    };
    executor.execute(vec![Box::new(install)], NonZeroUsize::MIN).await?;
    Ok(outcome)
}

/// Rebuild every `location` partition of one tenant's index from the store.
///
/// # Errors
///
/// Reports [`EntityError::Persistence`] when the store cannot be read, and
/// [`EntityError::Internal`] when the blocking worker does not complete the
/// install.
pub async fn rebuild_location_index(
    index: &Arc<SpatialEntityIndex>,
    source: &dyn EntitySpatialSnapshotSource,
    placement: &LocationPlacement,
    executor: &dyn BlockingExecutor,
) -> Result<RebuildOutcome, EntityError> {
    let snapshot = read_location_snapshot(index, source).await?;
    install_location_snapshot(index, placement, snapshot, executor, ()).await
}

/// One rebuild's install, run on a blocking worker.
struct Install<Held> {
    /// The index rebuilt.
    index: Arc<SpatialEntityIndex>,
    /// Which partitions an entity's location occupies.
    placement: LocationPlacement,
    /// Every located entity the store's read returned.
    locations: Vec<EntityLocation>,
    /// The index generation captured before the read.
    generation: u64,
    /// Whatever the caller keeps held until the install has finished.
    held: Held,
}

impl<Held: Send + 'static> BlockingTask for Install<Held> {
    fn run(self: Box<Self>) -> Result<(), EntityError> {
        let Install {
            index,
            placement,
            locations,
            generation,
            held,
        } = *self;
        let started = Instant::now();
        let mut partitions: HashMap<SpatialKey, Vec<SpatialEntry>> = HashMap::new();
        for location in locations {
            let entity_id = Arc::new(location.entity_id);
            for (key, geometry) in placement.of(&location.entity_types, location.location.as_ref().map(Geometry::form)) {
                partitions.entry(key).or_default().extend(SpatialEntry::new(Arc::clone(&entity_id), geometry));
            }
        }
        let partition_count = partitions.len();
        let total_entries: usize = partitions.values().map(Vec::len).sum();
        index.rebuild(placement.tenant(), placement.geoproperty(), partitions, generation);
        drop(held);
        info!(target: "spatial.rebuild", partition_count, total_entries, elapsed_ms = started.elapsed().as_millis(), "spatial index rebuilt the default location geoproperty");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityError,
        port::spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
        recording_ports::InlineBlockingExecutor,
        spatial_placement::LocationPlacement,
        spatial_reconciliation::rebuild_location_index,
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, geo_rel::GeoRel, geometry::Geometry, position::Position};
    use andromeda_spatial_index::{
        entity::index::{Admitted, SpatialEntityIndex},
        key::SpatialKey,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use futures::future::BoxFuture;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, num::NonZeroUsize, sync::Arc};

    /// A store holding one located vehicle.
    struct OneVehicle;

    impl EntitySpatialSnapshotSource for OneVehicle {
        fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async {
                Ok(LocationRead {
                    locations: vec![EntityLocation {
                        entity_id: "urn:ngsi-ld:Vehicle:stored".parse().expect("a well-formed URI"),
                        entity_types: vec!["https://example.org/Vehicle".parse().expect("a well-formed type")],
                        location: Some(Geometry::new(GeometryForm::Point(
                            Position::try_new(0.0, 0.0, None).expect("a test position lies on the CRS84 axes"),
                        ))),
                    }],
                    horizon: ChangeHorizon::new(7, vec![5]),
                })
            })
        }

        fn locations_changed_since<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _horizon: &'life1 ChangeHorizon,
        ) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { panic!("a rebuild reads no change") })
        }

        fn locations_changed_within<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _horizon: &'life1 ChangeHorizon,
            _bound: NonZeroUsize,
        ) -> BoxFuture<'async_trait, Result<BoundedChangeRead, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { panic!("a rebuild reads no change") })
        }
    }

    #[tokio::test]
    async fn a_rebuild_seeds_the_scope_with_what_the_store_holds() {
        let index = Arc::new(SpatialEntityIndex::new());
        let placement = LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name");
        let key = SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::new("https://example.org/Vehicle".to_owned()).expect("a well-formed IRI"),
            placement.geoproperty().clone(),
        );
        let reference = GeometryForm::Point(Position::try_new(0.0, 0.0, None).expect("a test position lies on the CRS84 axes"));
        assert!(index.query(&key, &GeoRel::Intersects, &reference, Admitted::Every, usize::MAX).is_none());

        let outcome = rebuild_location_index(&index, &OneVehicle, &placement, &InlineBlockingExecutor)
            .await
            .expect("the store is readable");

        assert_eq!((outcome.located_entities, outcome.horizon), (1, ChangeHorizon::new(7, vec![5])));
        assert_eq!(
            index.query(&key, &GeoRel::Intersects, &reference, Admitted::Every, usize::MAX),
            Some(HashSet::from(["urn:ngsi-ld:Vehicle:stored".parse().expect("a well-formed URI")]))
        );
    }
}
