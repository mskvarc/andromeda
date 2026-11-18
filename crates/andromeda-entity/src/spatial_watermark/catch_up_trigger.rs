//! When this broker process's own writes ask for a background catch-up.

use crate::port::spatial_catch_up_signal::SpatialCatchUpSignal;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// How many writes this process commits between two wakes of the background
/// catch-up.
///
/// Small enough that a query arriving after a load finds at most this many
/// changes waiting, which it takes in within milliseconds, and large enough
/// that a tenant written heavily and never geo-queried costs one bounded read
/// per this many writes rather than one per commit.
const WRITES_PER_WAKE: usize = 1_000;

/// Counts this process's writes since the last catch-up started, and wakes the
/// background catch-up every [`WRITES_PER_WAKE`] of them.
pub(super) struct CatchUpTrigger {
    /// What wakes the background catch-up.
    signal: Arc<dyn SpatialCatchUpSignal>,
    /// The writes committed since the last catch-up started reading.
    unread_writes: AtomicUsize,
}

impl CatchUpTrigger {
    /// A trigger waking the background catch-up through `signal`.
    pub(super) fn new(signal: Arc<dyn SpatialCatchUpSignal>) -> CatchUpTrigger {
        CatchUpTrigger {
            signal,
            unread_writes: AtomicUsize::new(0),
        }
    }

    /// Count `writes` this process committed, and wake the background
    /// catch-up when they complete another [`WRITES_PER_WAKE`].
    pub(super) fn writes_committed(&self, writes: usize) {
        let before = self.unread_writes.fetch_add(writes, Ordering::Relaxed);
        let after = before.saturating_add(writes);
        if before.div_euclid(WRITES_PER_WAKE) != after.div_euclid(WRITES_PER_WAKE) {
            self.signal.wake();
        }
    }

    /// Note that a catch-up is reading every write committed so far.
    pub(super) fn reading(&self) {
        self.unread_writes.store(0, Ordering::Relaxed);
    }

    /// Wake the background catch-up now.
    pub(super) fn wake(&self) {
        self.signal.wake();
    }
}
