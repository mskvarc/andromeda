//! Keeping the entity spatial index exact by catching it up from the store,
//! without making a geo query wait for more than a bounded catch-up.
//!
//! The index records the [`ChangeHorizon`] its last read of the store was
//! taken at, and holds every change below it. A geo query narrows through the
//! index only once every change committed before the query arrived is in.
//!
//! A query that finds the index current reads the changes above the horizon
//! itself, when no more than [`QUERY_CATCH_UP_BOUND`] entities changed, and
//! takes them in before it narrows. Catch-ups are coalesced. A query waits for
//! one that *started* after it arrived, since one started earlier may have read
//! the store before a write this query must see, and every query waiting when
//! one starts is served by it.
//!
//! When more entities changed, the index falls behind. The store answers every
//! geo query in full, which is exact, and the background catch-up takes the
//! backlog in with one unbounded read. This broker process's own writes wake
//! the background catch-up as they commit, so a load is taken in while it runs
//! and a query after it finds little to read.
//!
//! A rebuild holds nothing while it reads the store or installs its read, so
//! queries and catch-ups carry on throughout. Every write applied to the index
//! after the rebuild's generation was captured keeps its state over the
//! rebuild's read, so a rebuild never moves the horizon once the index is
//! seeded. A catch-up that
//! read the store before the rebuild did and applied its read after the
//! capture leaves an entity at an older state than the rebuild's read, and that
//! entity's later change is above the catch-up's horizon, so the next catch-up
//! reads it again. An entity deleted since stays in the index until the next
//! rebuild. That only ever widens a candidate set, since the store re-verifies
//! each candidate and drops it.

/// Taking a change read into the index on a blocking worker.
mod application;
/// When this broker process's own writes ask for a background catch-up.
mod catch_up_trigger;
/// Where the index stands against the store, and which queries it has served.
mod standing;
#[cfg(test)]
mod tests;

use crate::{
    error::EntityError,
    port::{
        blocking_executor::BlockingExecutor,
        spatial_catch_up_signal::SpatialCatchUpSignal,
        spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntitySpatialSnapshotSource, LocationRead},
    },
    spatial_placement::LocationPlacement,
    spatial_reconciliation::{RebuildOutcome, install_location_snapshot, read_location_snapshot},
    spatial_watermark::{
        application::{BacklogApplication, StepApplication},
        catch_up_trigger::CatchUpTrigger,
        standing::{Standing, WatermarkState},
    },
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use async_lock::{Mutex, MutexGuardArc};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// The most changed entities a query reads and takes in itself before it
/// narrows.
///
/// A query that finds more answers through the store in full and leaves them
/// to the background catch-up, so no query waits for more than this many.
pub const QUERY_CATCH_UP_BOUND: NonZeroUsize = NonZeroUsize::MIN.saturating_add(9_999);

/// What one background catch-up pass did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatchUpPass {
    /// The index holds every change the pass could read, or there was nothing
    /// to catch up.
    Settled,
    /// The pass took in a backlog with one long read, and changes committed
    /// while it read may be waiting above the new horizon.
    MoreMayRemain,
}

/// How a bounded step left the index.
enum Step {
    /// The index holds every change committed before the step's read.
    CaughtUp,
    /// More entities changed than a step takes in.
    Behind,
}

/// An entity spatial index caught up from the store.
pub struct SpatialWatermark {
    /// The index this watermark keeps exact.
    index: Arc<SpatialEntityIndex>,
    /// Where the index's entries are read from.
    source: Arc<dyn EntitySpatialSnapshotSource>,
    /// Which partitions an entity's location occupies.
    placement: LocationPlacement,
    /// Where the index takes reads in, off the async runtime.
    executor: Arc<dyn BlockingExecutor>,
    /// When this process's writes wake the background catch-up.
    trigger: CatchUpTrigger,
    /// The number of geo queries that have arrived so far.
    arrivals: AtomicU64,
    /// Where the index stands. Held across a bounded step, and by nothing that
    /// can take longer.
    state: Arc<Mutex<WatermarkState>>,
}

impl SpatialWatermark {
    /// A watermark over `index`, not yet seeded.
    #[must_use]
    pub fn new(
        index: Arc<SpatialEntityIndex>,
        source: Arc<dyn EntitySpatialSnapshotSource>,
        placement: LocationPlacement,
        executor: Arc<dyn BlockingExecutor>,
        signal: Arc<dyn SpatialCatchUpSignal>,
    ) -> SpatialWatermark {
        SpatialWatermark {
            index,
            source,
            placement,
            executor,
            trigger: CatchUpTrigger::new(signal),
            arrivals: AtomicU64::new(0),
            state: Arc::new(Mutex::new(WatermarkState::default())),
        }
    }

    /// Run `read` over the index when it holds every change the store
    /// committed before this call, or return `None` when the store must
    /// answer the query in full.
    ///
    /// Takes in at most [`QUERY_CATCH_UP_BOUND`] changed entities first. It
    /// never waits for a rebuild or for the background catch-up, and returns
    /// `None` at once while the background catch-up takes a backlog in or while
    /// more entities changed than the bound.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store cannot be read. The
    /// index is then left where it was, and must not narrow the query.
    pub async fn narrow<R>(&self, read: impl FnOnce(&SpatialEntityIndex) -> R) -> Result<Option<R>, EntityError> {
        let ticket = self.arrivals.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.state.lock_arc().await;
        if state.served_through >= ticket {
            drop(state);
            return Ok(Some(read(&self.index)));
        }
        match state.standing.clone() {
            Standing::Unseeded => Ok(None),
            Standing::Behind(_) => {
                drop(state);
                self.trigger.wake();
                Ok(None)
            }
            Standing::Current(horizon) => match self.step(state, horizon).await? {
                Step::CaughtUp => Ok(Some(read(&self.index))),
                Step::Behind => Ok(None),
            },
        }
    }

    /// Count `writes` this process committed to the store, waking the
    /// background catch-up as they accumulate.
    pub fn writes_committed(&self, writes: usize) {
        self.trigger.writes_committed(writes);
    }

    /// Run one background catch-up pass.
    ///
    /// A current index takes in what changed with one bounded step. An index
    /// with more waiting, or that such a step finds behind, takes the backlog
    /// in with one unbounded read, during which every geo query is answered by
    /// the store in full.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store cannot be read. An
    /// index behind stays behind.
    pub async fn catch_up(&self) -> Result<CatchUpPass, EntityError> {
        let state = self.state.lock_arc().await;
        let horizon = match state.standing.clone() {
            Standing::Unseeded => return Ok(CatchUpPass::Settled),
            Standing::Behind(horizon) => {
                drop(state);
                horizon
            }
            Standing::Current(horizon) => {
                // clone: the step hands the state to the worker that records its read
                match self.step(state, horizon.clone()).await? {
                    Step::CaughtUp => return Ok(CatchUpPass::Settled),
                    Step::Behind => horizon,
                }
            }
        };
        let round = self.arrivals.load(Ordering::SeqCst);
        self.trigger.reading();
        let read = self.source.locations_changed_since(&horizon).await?;
        let application = BacklogApplication {
            index: Arc::clone(&self.index),
            placement: self.placement.clone(), // clone: the blocking worker owns everything it reads
            read,
            round,
            state: Arc::clone(&self.state),
        };
        self.executor.execute(vec![Box::new(application)], NonZeroUsize::MIN).await?;
        Ok(CatchUpPass::MoreMayRemain)
    }

    /// Rebuild the index from the store, and seed it on its first rebuild.
    ///
    /// Neither the read nor the install holds a query or a catch-up back.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store cannot be read, and
    /// [`EntityError::Internal`] when the blocking worker does not complete the
    /// install.
    pub async fn rebuild(&self) -> Result<RebuildOutcome, EntityError> {
        let round = self.arrivals.load(Ordering::SeqCst);
        let snapshot = read_location_snapshot(&self.index, self.source.as_ref(), &self.placement).await?;
        let outcome = install_location_snapshot(&self.index, &self.placement, snapshot, self.executor.as_ref()).await?;
        // clone: the outcome reports the horizon the state records
        self.state.lock().await.seeded(outcome.horizon.clone(), round);
        Ok(outcome)
    }

    /// Read the changes above `horizon` when at most [`QUERY_CATCH_UP_BOUND`]
    /// entities changed, and take them in, holding `state` throughout.
    async fn step(&self, mut state: MutexGuardArc<WatermarkState>, horizon: ChangeHorizon) -> Result<Step, EntityError> {
        let round = self.arrivals.load(Ordering::SeqCst);
        self.trigger.reading();
        match self.source.locations_changed_within(&horizon, QUERY_CATCH_UP_BOUND).await? {
            BoundedChangeRead::Exceeded => {
                state.fell_behind();
                drop(state);
                self.trigger.wake();
                Ok(Step::Behind)
            }
            BoundedChangeRead::Within(read) => {
                self.take_in(state, read, round).await?;
                Ok(Step::CaughtUp)
            }
        }
    }

    /// Take a bounded step's read in and record it in `state`, on a blocking
    /// worker unless it changed nothing.
    async fn take_in(&self, mut state: MutexGuardArc<WatermarkState>, read: LocationRead, round: u64) -> Result<(), EntityError> {
        if read.locations.is_empty() {
            state.caught_up(read.horizon, round);
            return Ok(());
        }
        let application = StepApplication {
            index: Arc::clone(&self.index),
            placement: self.placement.clone(), // clone: the blocking worker owns everything it reads
            read,
            round,
            state,
        };
        self.executor.execute(vec![Box::new(application)], NonZeroUsize::MIN).await
    }
}
