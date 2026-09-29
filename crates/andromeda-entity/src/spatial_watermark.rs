//! Keeping the entity spatial index exact by catching it up from the store
//! before it narrows a query.
//!
//! The index records the [`ChangeHorizon`] its last read of the store was
//! taken at. Before narrowing, the query path reads every entity the store
//! changed at or above that horizon, applies them, and advances the horizon.
//! Every change that committed before the query arrived is then in the index,
//! whichever broker process made it, so the index is complete for every entity
//! a geo query could match. An entity deleted since stays in the index until
//! the next rebuild, which only ever widens a candidate set: the store
//! re-verifies each candidate and drops it.
//!
//! Catch-ups are coalesced. A query waits for a catch-up that *started* after
//! it arrived -- one started earlier may have read the store before a write
//! this query must see -- and every query waiting when a catch-up starts is
//! served by it, so a burst of geo queries costs one read of the store rather
//! than one each.

use crate::{
    error::EntityError,
    port::spatial_snapshot::{ChangeHorizon, EntitySpatialSnapshotSource},
    spatial_placement::LocationPlacement,
    spatial_reconciliation::{RebuildOutcome, rebuild_location_index},
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use async_lock::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Where the index stands against the store.
#[derive(Debug, Default)]
struct WatermarkState {
    /// The horizon of the last read applied, or `None` before the first
    /// rebuild seeded the index.
    horizon: Option<ChangeHorizon>,
    /// The arrival count at the start of the last completed catch-up: every
    /// query that arrived at or below it has been served.
    served_through: u64,
}

/// An entity spatial index caught up from the store on demand.
pub struct SpatialWatermark {
    /// The index this watermark keeps exact.
    index: Arc<SpatialEntityIndex>,
    /// Where the index's entries are read from.
    source: Arc<dyn EntitySpatialSnapshotSource>,
    /// Which partitions an entity's location occupies.
    placement: LocationPlacement,
    /// The number of catch-ups requested so far.
    arrivals: AtomicU64,
    /// The index's position against the store; held for the whole of a
    /// catch-up or rebuild, which is what serialises every write to the index.
    state: Mutex<WatermarkState>,
}

impl SpatialWatermark {
    /// A watermark over `index`, not yet seeded.
    #[must_use]
    pub fn new(index: Arc<SpatialEntityIndex>, source: Arc<dyn EntitySpatialSnapshotSource>, placement: LocationPlacement) -> SpatialWatermark {
        SpatialWatermark {
            index,
            source,
            placement,
            arrivals: AtomicU64::new(0),
            state: Mutex::new(WatermarkState::default()),
        }
    }

    /// The index this watermark keeps exact.
    #[must_use]
    pub const fn index(&self) -> &Arc<SpatialEntityIndex> {
        &self.index
    }

    /// Bring the index up to every change the store committed before this call.
    ///
    /// Returns at once when a catch-up that started after this call has
    /// already completed, and does nothing before the first rebuild: an
    /// unseeded index declines every query anyway.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store cannot be read. The
    /// index is then left where it was, and must not narrow the query.
    pub async fn catch_up(&self) -> Result<(), EntityError> {
        let ticket = self.arrivals.fetch_add(1, Ordering::SeqCst) + 1;
        let mut state = self.state.lock().await;
        if state.served_through >= ticket {
            return Ok(());
        }
        let Some(horizon) = state.horizon else {
            return Ok(());
        };
        let round = self.arrivals.load(Ordering::SeqCst);
        let read = self.source.locations_changed_since(horizon).await?;
        for location in read.locations {
            let placements = self.placement.of(&location.entity_types, location.location.as_ref());
            self.index.replace_entity(location.entity_id, placements);
        }
        state.horizon = Some(read.horizon);
        state.served_through = round;
        Ok(())
    }

    /// Rebuild the index from the store and record the horizon of the read.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store cannot be read.
    pub async fn rebuild(&self) -> Result<RebuildOutcome, EntityError> {
        let mut state = self.state.lock().await;
        let round = self.arrivals.load(Ordering::SeqCst);
        let outcome = rebuild_location_index(&self.index, self.source.as_ref(), &self.placement).await?;
        state.horizon = Some(outcome.horizon);
        state.served_through = round;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityError,
        port::spatial_snapshot::{ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
        spatial_placement::LocationPlacement,
        spatial_watermark::SpatialWatermark,
    };
    use andromeda_ngsi_ld_geometry::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        geometry::Geometry,
        position::Position,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_spatial_index::{entity::index::SpatialEntityIndex, key::SpatialKey};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use futures::future::BoxFuture;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };

    /// A store whose rows another broker process changes behind this one's
    /// back: every row carries the horizon it was written at.
    #[derive(Default)]
    struct SharedStore {
        /// `(written at, identifier, x)` for every stored vehicle.
        rows: Mutex<Vec<(u64, &'static str, f64)>>,
        /// The next horizon a write is stamped with.
        clock: Mutex<u64>,
        /// Every horizon a change read was asked for.
        change_reads: Mutex<Vec<u64>>,
    }

    impl SharedStore {
        fn write(&self, id: &'static str, x: f64) {
            let mut clock = self.clock.lock().expect("clock lock");
            *clock += 1;
            let mut rows = self.rows.lock().expect("rows lock");
            rows.retain(|(_, stored, _)| *stored != id);
            rows.push((*clock, id, x));
        }

        fn read(&self, since: u64) -> LocationRead {
            let clock = *self.clock.lock().expect("clock lock");
            let locations = self
                .rows
                .lock()
                .expect("rows lock")
                .iter()
                .filter(|(written, _, _)| *written >= since)
                .map(|(_, id, x)| EntityLocation {
                    entity_id: id.parse().expect("a well-formed URI"),
                    entity_types: vec!["https://example.org/Vehicle".parse().expect("a well-formed type")],
                    location: Some(point(*x)),
                })
                .collect();
            LocationRead {
                locations,
                horizon: ChangeHorizon::new(clock + 1),
            }
        }
    }

    impl EntitySpatialSnapshotSource for SharedStore {
        fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move { Ok(self.read(0)) })
        }

        fn locations_changed_since<'life0, 'async_trait>(&'life0 self, horizon: ChangeHorizon) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                self.change_reads.lock().expect("reads lock").push(horizon.position());
                Ok(self.read(horizon.position()))
            })
        }
    }

    fn point(x: f64) -> Geometry {
        Geometry::new(GeometryForm::Point(Position::new(x, 0.0, None)))
    }

    fn watermark(store: &Arc<SharedStore>) -> SpatialWatermark {
        SpatialWatermark::new(
            Arc::new(SpatialEntityIndex::new()),
            Arc::clone(store) as Arc<dyn EntitySpatialSnapshotSource>, // clone: the watermark and the test share the store
            LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name"),
        )
    }

    fn near_origin(watermark: &SpatialWatermark) -> Option<HashSet<Identifier>> {
        let key = SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::new("https://example.org/Vehicle".to_owned()).expect("a well-formed IRI"),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        );
        let square = Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![vec![
            Position::new(-1.0, -1.0, None),
            Position::new(1.0, -1.0, None),
            Position::new(1.0, 1.0, None),
            Position::new(-1.0, 1.0, None),
            Position::new(-1.0, -1.0, None),
        ]])));
        watermark.index().query(&key, &GeoRel::Within, &square)
    }

    #[tokio::test]
    async fn a_catch_up_finds_an_entity_another_process_moved_into_range() {
        let store = Arc::new(SharedStore::default());
        store.write("urn:ngsi-ld:Vehicle:far", 50.0);
        let watermark = watermark(&store);
        watermark.rebuild().await.expect("the store is readable");
        assert_eq!(near_origin(&watermark), Some(HashSet::new()));

        store.write("urn:ngsi-ld:Vehicle:far", 0.5);
        watermark.catch_up().await.expect("the store is readable");

        assert_eq!(
            near_origin(&watermark),
            Some(HashSet::from(["urn:ngsi-ld:Vehicle:far".parse().expect("a well-formed URI")]))
        );
    }

    #[tokio::test]
    async fn a_catch_up_reads_only_the_changes_above_the_last_horizon() {
        let store = Arc::new(SharedStore::default());
        store.write("urn:ngsi-ld:Vehicle:a", 0.0);
        let watermark = watermark(&store);
        watermark.rebuild().await.expect("the store is readable");
        store.write("urn:ngsi-ld:Vehicle:b", 0.2);
        watermark.catch_up().await.expect("the store is readable");
        store.write("urn:ngsi-ld:Vehicle:c", 0.3);
        watermark.catch_up().await.expect("the store is readable");

        assert_eq!(*store.change_reads.lock().expect("reads lock"), vec![2, 3]);
        assert_eq!(near_origin(&watermark).map(|found| found.len()), Some(3));
    }

    #[tokio::test]
    async fn nothing_is_caught_up_before_the_first_rebuild() {
        let store = Arc::new(SharedStore::default());
        store.write("urn:ngsi-ld:Vehicle:a", 0.0);
        let watermark = watermark(&store);
        watermark.catch_up().await.expect("the store is readable");
        assert!(store.change_reads.lock().expect("reads lock").is_empty());
        assert_eq!(near_origin(&watermark), None);
    }
}
