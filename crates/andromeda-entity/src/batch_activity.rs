//! Batch-activity tracking for coordinating background workers with bulk ingest.
//!
//! During high-throughput batch operations the data expiration worker's periodic
//! scans contend for I/O and accomplish nothing (no rows expire during ingest).
//! This module provides a lightweight signalling mechanism: the batch path
//! records the wall-clock time of its last activity, and background workers
//! check whether a configurable cooldown has elapsed before proceeding.

use std::{
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

/// Default cooldown after the last batch operation before background workers
/// resume their scans.
const DEFAULT_COOLDOWN: Duration = Duration::from_secs(10);

/// Records the timestamp of the most recent batch operation so that background
/// workers can back off during (and shortly after) bulk ingest.
///
/// Internally stores the UNIX epoch timestamp (seconds) of the last activity.
/// A value of `0` means no batch activity has been recorded yet.
#[derive(Debug, Clone)]
pub struct BatchActivityTracker {
    last_activity_epoch: Arc<AtomicI64>,
    cooldown: Duration,
}

impl BatchActivityTracker {
    /// Creates a new tracker with the default 10-second cooldown.
    #[must_use]
    pub fn new() -> BatchActivityTracker {
        BatchActivityTracker {
            last_activity_epoch: Arc::new(AtomicI64::new(0)),
            cooldown: DEFAULT_COOLDOWN,
        }
    }

    /// Records the current wall-clock time as the most recent batch activity.
    ///
    /// Called by the batch ingest path at the start of each batch operation.
    pub fn record_activity(&self) {
        let now = chrono::Utc::now().timestamp();
        self.last_activity_epoch.store(now, Ordering::Release);
    }

    /// Returns `true` if a batch operation is in progress or completed less
    /// than [`cooldown`](Self::new) seconds ago.
    ///
    /// Background workers should skip their work when this returns `true`.
    #[must_use]
    pub fn is_within_cooldown(&self) -> bool {
        let last = self.last_activity_epoch.load(Ordering::Acquire);
        if last == 0 {
            return false;
        }
        let now = chrono::Utc::now().timestamp();
        match u64::try_from(now.saturating_sub(last)) {
            Ok(elapsed) => Duration::from_secs(elapsed) < self.cooldown,
            // The wall clock moved backwards past the recorded activity: hold
            // the window open rather than releasing every worker at once.
            Err(_) => true,
        }
    }
}

// Deriving `Default` would select a zero cooldown and disable the intended back-pressure window.
impl Default for BatchActivityTracker {
    fn default() -> BatchActivityTracker {
        BatchActivityTracker {
            last_activity_epoch: Arc::new(AtomicI64::new(0)),
            cooldown: DEFAULT_COOLDOWN,
        }
    }
}
