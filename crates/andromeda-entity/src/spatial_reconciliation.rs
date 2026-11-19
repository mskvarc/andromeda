//! Rebuilding the in-memory entity spatial index from a read of the store.
//!
//! A rebuild is two phases. The read holds nothing, so geo queries and every
//! other writer of the index carry on while the store is read. The store hands
//! its located entities in a batch at a time, and each entity is placed into
//! the partitions it occupies as its batch arrives, so the read holds the
//! entries it built and one batch, never every entity. A read that fails part
//! way drops what it built and leaves the index as it was. The install
//! bulk-loads every partition, which takes seconds for millions of entities,
//! so it runs on a blocking worker rather than on the async runtime.

use crate::{
    error::EntityError,
    port::{
        blocking_executor::{BlockingExecutor, BlockingTask},
        spatial_snapshot::{ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationSink},
    },
    spatial_placement::LocationPlacement,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::identifier_spelling::IdentifierSpelling;
use andromeda_spatial_index::{
    entity::{entry::SpatialEntry, index::SpatialEntityIndex},
    key::SpatialKey,
};
use std::{
    collections::HashMap,
    mem,
    num::NonZeroUsize,
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};
use tracing::info;

/// Result of a rebuild pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildOutcome {
    /// Number of located entities the store's read returned.
    pub located_entities: usize,
    /// The horizon the store's read was taken at.
    pub horizon: ChangeHorizon,
}

/// The partitions a rebuild installs, and how many located entities were
/// placed into them.
#[derive(Debug, Default)]
struct Partitions {
    /// Every partition's entries.
    entries: HashMap<SpatialKey, Vec<SpatialEntry>>,
    /// Number of located entities the store's read returned.
    located_entities: usize,
}

/// The partitions of a rebuild, filled as the store's read hands its located
/// entities in.
///
/// An entity's types and geometry are dropped once its entries exist. Its
/// entries keep the geometry's projection and one identifier they share.
struct PartitionFill {
    /// Which partitions an entity's location occupies.
    placement: LocationPlacement,
    /// The partitions filled so far. A batch is taken in under the lock, by
    /// whichever thread the store decoded it on.
    partitions: Mutex<Partitions>,
}

impl PartitionFill {
    /// Everything filled so far, leaving the fill empty.
    fn drain(&self) -> Partitions {
        mem::take(&mut *self.partitions.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl LocationSink for PartitionFill {
    fn take(&self, locations: Vec<EntityLocation>) {
        let mut partitions = self.partitions.lock().unwrap_or_else(PoisonError::into_inner);
        partitions.located_entities = partitions.located_entities.saturating_add(locations.len());
        for location in locations {
            let entity_id = IdentifierSpelling::from(&location.entity_id);
            for (key, geometry) in self.placement.of(&location.entity_types, location.location.as_ref().map(Geometry::form)) {
                partitions
                    .entries
                    .entry(key)
                    .or_default()
                    .extend(SpatialEntry::new(entity_id.clone(), geometry));
            }
        }
    }
}

/// A read of every located entity, placed into the partitions each occupies,
/// and the index generation captured before it was taken.
#[derive(Debug)]
pub struct LocationSnapshot {
    /// The index generation captured before the read. Every write applied to
    /// the index after it keeps its state over the read's.
    generation: u64,
    /// The horizon the store's read was taken at.
    horizon: ChangeHorizon,
    /// The partitions the read's located entities occupy.
    partitions: Partitions,
}

/// Read every located entity of the store for a rebuild of `index`, placing
/// each into the partitions `placement` gives it as the store hands it in.
///
/// The generation is captured before the read, so a write applied to the
/// index while the store is read keeps its state over the read's.
///
/// # Errors
///
/// Reports [`EntityError::Persistence`] when the store cannot be read. What
/// the read placed before it failed is dropped.
pub async fn read_location_snapshot(
    index: &SpatialEntityIndex,
    source: &dyn EntitySpatialSnapshotSource,
    placement: &LocationPlacement,
) -> Result<LocationSnapshot, EntityError> {
    let generation = index.current_generation();
    let fill = Arc::new(PartitionFill {
        placement: placement.clone(), // clone: the store takes batches in on threads of its own
        partitions: Mutex::default(),
    });
    let horizon = source.locations(Arc::clone(&fill) as Arc<dyn LocationSink>).await?;
    Ok(LocationSnapshot {
        generation,
        horizon,
        partitions: fill.drain(),
    })
}

/// Rebuild every `location` partition of one tenant's index from `snapshot`,
/// and seed the scope so the index may narrow queries against it.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the blocking worker does not
/// complete the install.
pub async fn install_location_snapshot(
    index: &Arc<SpatialEntityIndex>,
    placement: &LocationPlacement,
    snapshot: LocationSnapshot,
    executor: &dyn BlockingExecutor,
) -> Result<RebuildOutcome, EntityError> {
    let LocationSnapshot {
        generation,
        horizon,
        partitions,
    } = snapshot;
    let outcome = RebuildOutcome {
        located_entities: partitions.located_entities,
        horizon,
    };
    let install = Install {
        index: Arc::clone(index),
        placement: placement.clone(), // clone: the blocking worker owns everything it reads
        entries: partitions.entries,
        generation,
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
    let snapshot = read_location_snapshot(index, source, placement).await?;
    install_location_snapshot(index, placement, snapshot, executor).await
}

/// One rebuild's install, run on a blocking worker.
struct Install {
    /// The index rebuilt.
    index: Arc<SpatialEntityIndex>,
    /// The tenant and the geoproperty whose partitions are rebuilt.
    placement: LocationPlacement,
    /// Every partition's entries.
    entries: HashMap<SpatialKey, Vec<SpatialEntry>>,
    /// The index generation captured before the read.
    generation: u64,
}

impl BlockingTask for Install {
    fn run(self: Box<Self>) -> Result<(), EntityError> {
        let Install {
            index,
            placement,
            entries,
            generation,
        } = *self;
        let started = Instant::now();
        let partition_count = entries.len();
        let total_entries: usize = entries.values().map(Vec::len).sum();
        index.rebuild(placement.tenant(), placement.geoproperty(), entries, generation);
        info!(target: "spatial.rebuild", partition_count, total_entries, elapsed_ms = started.elapsed().as_millis(), "spatial index rebuilt the default location geoproperty");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityError,
        port::spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead, LocationSink},
        recording_ports::InlineBlockingExecutor,
        spatial_placement::LocationPlacement,
        spatial_reconciliation::{read_location_snapshot, rebuild_location_index},
    };
    use andromeda_ngsi_ld_geometry::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        geometry::Geometry,
        linear_ring::LinearRing,
        position::Position,
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, identifier_spelling::IdentifierSpelling};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_spatial_index::{
        entity::{
            entry::SpatialEntry,
            index::{Admitted, SpatialEntityIndex},
        },
        key::SpatialKey,
        partitioned_rtree::Indexed,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use futures::future::BoxFuture;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, num::NonZeroUsize, sync::Arc};

    /// How a [`Stored`] read ends.
    #[derive(Clone, Copy)]
    enum ReadEnd {
        /// The read returns the horizon it was taken at.
        Completes,
        /// The store becomes unreachable once the first batch is handed in.
        FailsAfterTheFirstBatch,
    }

    /// A store holding the entities `entities` gives, handing them in `batch` at
    /// a time.
    struct Stored {
        entities: fn() -> Vec<EntityLocation>,
        batch: usize,
        end: ReadEnd,
    }

    impl Stored {
        fn handing(entities: fn() -> Vec<EntityLocation>, batch: usize) -> Stored {
            Stored {
                entities,
                batch,
                end: ReadEnd::Completes,
            }
        }
    }

    impl EntitySpatialSnapshotSource for Stored {
        fn locations<'life0, 'async_trait>(&'life0 self, sink: Arc<dyn LocationSink>) -> BoxFuture<'async_trait, Result<ChangeHorizon, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut remaining = (self.entities)();
                while !remaining.is_empty() {
                    let rest = remaining.split_off(self.batch.min(remaining.len()));
                    sink.take(remaining);
                    remaining = rest;
                    match self.end {
                        ReadEnd::Completes => {}
                        ReadEnd::FailsAfterTheFirstBatch => return Err(EntityError::from(PersistenceFailure::Unavailable)),
                    }
                }
                Ok(ChangeHorizon::new(7, vec![5]))
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

    fn position(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn point(longitude: f64) -> GeometryForm {
        GeometryForm::Point(position(longitude, 0.0))
    }

    /// The square two degrees wide centred on `(longitude, 0)`.
    fn square(longitude: f64) -> GeometryForm {
        GeometryForm::Polygon(PolygonRings::new(vec![
            LinearRing::try_new(vec![
                position(longitude - 1.0, -1.0),
                position(longitude + 1.0, -1.0),
                position(longitude + 1.0, 1.0),
                position(longitude - 1.0, 1.0),
                position(longitude - 1.0, -1.0),
            ])
            .expect("a test square is a closed ring"),
        ]))
    }

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("a well-formed URI")
    }

    fn spelled(raw: &str) -> IdentifierSpelling {
        IdentifierSpelling::from(&id(raw))
    }

    fn located(raw_id: &str, types: &[&str], location: GeometryForm) -> EntityLocation {
        EntityLocation {
            entity_id: id(raw_id),
            entity_types: types.iter().map(|name| name.parse().expect("a well-formed type")).collect(),
            location: Some(Geometry::new(location)),
        }
    }

    /// One vehicle at the origin.
    fn one_vehicle() -> Vec<EntityLocation> {
        vec![located("urn:ngsi-ld:Vehicle:stored", &["https://example.org/Vehicle"], point(0.0))]
    }

    /// A vehicle, a vehicle that is also an asset, and an asset whose other
    /// type is not expanded, so names no partition a query reads.
    fn fleet() -> Vec<EntityLocation> {
        vec![
            located("urn:ngsi-ld:Vehicle:a", &["https://example.org/Vehicle"], point(10.0)),
            located(
                "urn:ngsi-ld:Vehicle:b",
                &["https://example.org/Vehicle", "https://example.org/Asset"],
                square(20.0),
            ),
            located("urn:ngsi-ld:Asset:c", &["Thing", "https://example.org/Asset"], point(30.0)),
        ]
    }

    fn placement() -> LocationPlacement {
        LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name")
    }

    fn key(type_iri: &str) -> SpatialKey {
        SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::new(type_iri.to_owned()).expect("a well-formed IRI"),
            placement().geoproperty().clone(),
        )
    }

    /// The vehicles the index holds within a degree of `(longitude, 0)`.
    fn vehicles_near(index: &SpatialEntityIndex, longitude: f64) -> Option<HashSet<IdentifierSpelling>> {
        index.query(
            &key("https://example.org/Vehicle"),
            &GeoRel::Within,
            &square(longitude),
            Admitted::Every,
            usize::MAX,
        )
    }

    #[tokio::test]
    async fn a_rebuild_seeds_the_scope_with_what_the_store_holds() {
        let index = Arc::new(SpatialEntityIndex::new());
        assert!(vehicles_near(&index, 0.0).is_none());

        let outcome = rebuild_location_index(&index, &Stored::handing(one_vehicle, 1), &placement(), &InlineBlockingExecutor)
            .await
            .expect("the store is readable");

        assert_eq!((outcome.located_entities, outcome.horizon), (1, ChangeHorizon::new(7, vec![5])));
        assert_eq!(vehicles_near(&index, 0.0), Some(HashSet::from([spelled("urn:ngsi-ld:Vehicle:stored")])));
    }

    #[tokio::test]
    async fn a_read_handed_in_batches_fills_the_same_partitions_as_one_collected_batch() {
        let index = SpatialEntityIndex::new();
        let collected = read_location_snapshot(&index, &Stored::handing(fleet, usize::MAX), &placement())
            .await
            .expect("the store is readable");
        let ids_in = |entries: &Vec<SpatialEntry>| entries.iter().map(|entry| entry.id().clone()).collect::<Vec<_>>();
        assert_eq!(collected.partitions.located_entities, 3);
        assert_eq!(collected.partitions.entries.len(), 2, "the unexpanded type names no partition");
        assert_eq!(
            collected.partitions.entries.get(&key("https://example.org/Vehicle")).map(ids_in),
            Some(vec![spelled("urn:ngsi-ld:Vehicle:a"), spelled("urn:ngsi-ld:Vehicle:b")])
        );
        assert_eq!(
            collected.partitions.entries.get(&key("https://example.org/Asset")).map(ids_in),
            Some(vec![spelled("urn:ngsi-ld:Vehicle:b"), spelled("urn:ngsi-ld:Asset:c")])
        );

        for batch in [1, 2] {
            let streamed = read_location_snapshot(&index, &Stored::handing(fleet, batch), &placement())
                .await
                .expect("the store is readable");
            assert_eq!(
                streamed.partitions.located_entities, collected.partitions.located_entities,
                "batches of {batch}"
            );
            assert_eq!(streamed.partitions.entries, collected.partitions.entries, "batches of {batch}");
            assert_eq!(streamed.horizon, collected.horizon, "batches of {batch}");
        }
    }

    #[tokio::test]
    async fn a_read_that_fails_part_way_leaves_the_index_as_it_was() {
        let index = Arc::new(SpatialEntityIndex::new());
        rebuild_location_index(&index, &Stored::handing(one_vehicle, 1), &placement(), &InlineBlockingExecutor)
            .await
            .expect("the store is readable");
        let failing = Stored {
            entities: fleet,
            batch: 1,
            end: ReadEnd::FailsAfterTheFirstBatch,
        };

        let failed = rebuild_location_index(&index, &failing, &placement(), &InlineBlockingExecutor).await;

        assert!(matches!(failed, Err(EntityError::StoreUnavailable)), "{failed:?}");
        assert_eq!(vehicles_near(&index, 0.0), Some(HashSet::from([spelled("urn:ngsi-ld:Vehicle:stored")])));
        assert_eq!(
            vehicles_near(&index, 10.0),
            Some(HashSet::new()),
            "nothing the failed read handed in is installed"
        );
    }
}
