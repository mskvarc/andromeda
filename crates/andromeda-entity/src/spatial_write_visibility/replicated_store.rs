//! The store another broker process writes, which a watermark catches up from.

use crate::{
    error::EntityError,
    port::spatial_snapshot::{ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
    spatial_write_visibility::geo_query::{VEHICLE_TYPE, point, uri},
};
use futures::future::BoxFuture;
use std::sync::Mutex;

/// The store another broker process writes: every change carries the
/// horizon it was committed at, and the index learns of it only by reading.
#[derive(Default)]
pub(super) struct ReplicatedStore {
    /// `(committed at, identifier, longitude)` for every stored vehicle.
    rows: Mutex<Vec<(u64, &'static str, f64)>>,
    /// The horizon the next change is committed at.
    clock: Mutex<u64>,
    /// Whether reads fail, as when the store is unreachable.
    pub(super) unreachable: Mutex<bool>,
}

impl ReplicatedStore {
    /// Commit a vehicle at `longitude`, as the other broker process does.
    pub(super) fn write(&self, id: &'static str, longitude: f64) {
        let mut clock = self.clock.lock().expect("the clock lock is not poisoned");
        *clock += 1;
        let mut rows = self.rows.lock().expect("the rows lock is not poisoned");
        rows.retain(|(_, stored, _)| *stored != id);
        rows.push((*clock, id, longitude));
    }

    fn read(&self, since: u64) -> Result<LocationRead, EntityError> {
        if *self.unreachable.lock().expect("the reachability lock is not poisoned") {
            return Err(EntityError::persistence("the store is unreachable"));
        }
        let horizon = *self.clock.lock().expect("the clock lock is not poisoned") + 1;
        let locations = self
            .rows
            .lock()
            .expect("the rows lock is not poisoned")
            .iter()
            .filter(|(committed, _, _)| *committed >= since)
            .map(|(_, id, longitude)| EntityLocation {
                entity_id: uri(id),
                entity_types: vec![VEHICLE_TYPE.parse().expect("a well-formed type")],
                location: Some(point(*longitude)),
            })
            .collect();
        Ok(LocationRead {
            locations,
            horizon: ChangeHorizon::new(horizon),
        })
    }
}

impl EntitySpatialSnapshotSource for ReplicatedStore {
    fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { self.read(0) })
    }

    fn locations_changed_since<'life0, 'async_trait>(&'life0 self, horizon: ChangeHorizon) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { self.read(horizon.position()) })
    }
}
