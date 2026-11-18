//! The store another broker process writes, which a watermark catches up from.

use crate::{
    error::EntityError,
    port::spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
    spatial_write_visibility::geo_query::{VEHICLE_TYPE, point, uri},
};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use futures::{channel::oneshot, future::BoxFuture};
use std::{num::NonZeroUsize, sync::Mutex};

/// One vehicle row, and the transaction that wrote it.
#[derive(Clone)]
struct Row {
    /// The position of the transaction that wrote the row.
    stamp: u64,
    /// The vehicle's identifier.
    id: String,
    /// The vehicle's longitude on the equator.
    longitude: f64,
}

/// One read the store answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// A read of every located vehicle.
    Full,
    /// An unbounded read of the changes above this horizon.
    Since(ChangeHorizon),
    /// A bounded read of the changes above this horizon.
    Within(ChangeHorizon),
}

/// The store another broker process writes. Every row carries the position of
/// the transaction that wrote it, and the index learns of it only by reading.
///
/// A read's horizon is cut the way PostgreSQL's snapshot is: its ceiling is
/// the next position to be handed out, and the positions of the transactions
/// still open are unseen below it.
#[derive(Default)]
pub(crate) struct ReplicatedStore {
    /// Every committed row, one per vehicle.
    rows: Mutex<Vec<Row>>,
    /// The rows of transactions not committed yet.
    open: Mutex<Vec<Row>>,
    /// The last position handed out.
    clock: Mutex<u64>,
    /// Whether reads fail, as when the store is unreachable.
    pub(crate) unreachable: Mutex<bool>,
    /// Every read answered, and how many vehicles it returned.
    reads: Mutex<Vec<(Read, usize)>>,
    /// Holds the next full read's answer until the sender is used or dropped.
    held_full_read: Mutex<Option<oneshot::Receiver<()>>>,
}

impl ReplicatedStore {
    /// Commit a vehicle at `longitude` in a transaction of its own, as the
    /// other broker process does.
    pub(crate) fn write(&self, id: &str, longitude: f64) {
        let stamp = self.next_position();
        self.commit_rows(vec![Row {
            stamp,
            id: id.to_owned(),
            longitude,
        }]);
    }

    /// Commit `count` vehicles named `<prefix>-<n>` at `longitude` in one
    /// transaction.
    pub(crate) fn write_many(&self, prefix: &str, count: usize, longitude: f64) {
        let stamp = self.next_position();
        self.commit_rows(
            (0..count)
                .map(|n| Row {
                    stamp,
                    id: format!("{prefix}-{n}"),
                    longitude,
                })
                .collect(),
        );
    }

    /// Write a vehicle at `longitude` in a transaction left open, and return
    /// its position.
    pub(crate) fn begin(&self, id: &str, longitude: f64) -> u64 {
        let stamp = self.next_position();
        self.open.lock().expect("the open lock is not poisoned").push(Row {
            stamp,
            id: id.to_owned(),
            longitude,
        });
        stamp
    }

    /// Commit the open transaction at `stamp`.
    pub(crate) fn commit(&self, stamp: u64) {
        let rows = self
            .open
            .lock()
            .expect("the open lock is not poisoned")
            .extract_if(.., |row| row.stamp == stamp)
            .collect();
        self.commit_rows(rows);
    }

    /// Hold the answer of the next full read until the returned sender is
    /// used or dropped. The read is taken when it is asked for.
    pub(crate) fn hold_next_full_read(&self) -> oneshot::Sender<()> {
        let (release, held) = oneshot::channel();
        *self.held_full_read.lock().expect("the hold lock is not poisoned") = Some(held);
        release
    }

    /// Every read answered so far, and how many vehicles each returned.
    pub(crate) fn reads(&self) -> Vec<(Read, usize)> {
        self.reads.lock().expect("the reads lock is not poisoned").clone()
    }

    fn next_position(&self) -> u64 {
        let mut clock = self.clock.lock().expect("the clock lock is not poisoned");
        *clock += 1;
        *clock
    }

    fn commit_rows(&self, committed: Vec<Row>) {
        let mut rows = self.rows.lock().expect("the rows lock is not poisoned");
        rows.retain(|row| committed.iter().all(|new| new.id != row.id));
        rows.extend(committed);
    }

    /// The horizon a read taken now is cut at.
    fn horizon(&self) -> ChangeHorizon {
        let ceiling = *self.clock.lock().expect("the clock lock is not poisoned") + 1;
        let unseen = self.open.lock().expect("the open lock is not poisoned").iter().map(|row| row.stamp).collect();
        ChangeHorizon::new(ceiling, unseen)
    }

    /// Answer `read` with the committed vehicles `above` selects.
    fn answer(&self, read: Read, above: Option<&ChangeHorizon>) -> Result<LocationRead, EntityError> {
        if *self.unreachable.lock().expect("the reachability lock is not poisoned") {
            return Err(EntityError::from(PersistenceFailure::Unavailable));
        }
        let horizon = self.horizon();
        let locations: Vec<EntityLocation> = self
            .rows
            .lock()
            .expect("the rows lock is not poisoned")
            .iter()
            .filter(|row| above.is_none_or(|cut| row.stamp >= cut.ceiling() || cut.unseen().contains(&row.stamp)))
            .map(|row| EntityLocation {
                entity_id: uri(&row.id),
                entity_types: vec![VEHICLE_TYPE.parse().expect("a well-formed type")],
                location: Some(point(row.longitude)),
            })
            .collect();
        self.reads.lock().expect("the reads lock is not poisoned").push((read, locations.len()));
        Ok(LocationRead { locations, horizon })
    }
}

impl EntitySpatialSnapshotSource for ReplicatedStore {
    fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        let read = self.answer(Read::Full, None);
        let held = self.held_full_read.lock().expect("the hold lock is not poisoned").take();
        Box::pin(async move {
            if let Some(held) = held {
                let _released = held.await;
            }
            read
        })
    }

    fn locations_changed_since<'life0, 'life1, 'async_trait>(
        &'life0 self,
        horizon: &'life1 ChangeHorizon,
    ) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { self.answer(Read::Since(horizon.clone()), Some(horizon)) })
    }

    fn locations_changed_within<'life0, 'life1, 'async_trait>(
        &'life0 self,
        horizon: &'life1 ChangeHorizon,
        bound: NonZeroUsize,
    ) -> BoxFuture<'async_trait, Result<BoundedChangeRead, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let read = self.answer(Read::Within(horizon.clone()), Some(horizon))?;
            Ok(if read.locations.len() > bound.get() {
                BoundedChangeRead::Exceeded
            } else {
                BoundedChangeRead::Within(read)
            })
        })
    }
}
