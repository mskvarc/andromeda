//! Background worker that periodically deletes expired snapshots.
//!
//! Per NGSI-LD 5.16, snapshots have an `expiresAt` timestamp derived from
//! `snapshotLifetime`. This worker cleans up expired snapshots to prevent
//! unbounded storage growth.

use crate::repository::snapshot::repo::SnapshotRepository;
use std::time::Duration;
use tracing::{debug, error, info};

/// Periodic worker that removes expired snapshots from the database.
pub struct SnapshotExpirationWorker {
    repo: SnapshotRepository,
    interval: Duration,
}

impl SnapshotExpirationWorker {
    /// Create a new worker with the given interval between expiration sweeps.
    pub const fn new(repo: SnapshotRepository, interval_secs: u64) -> SnapshotExpirationWorker {
        SnapshotExpirationWorker {
            repo,
            interval: Duration::from_secs(interval_secs),
        }
    }

    /// Run the expiration loop indefinitely.
    pub async fn run(self) {
        info!("SnapshotExpirationWorker starting (interval: {:?})", self.interval);
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            match self.repo.delete_expired().await {
                Ok(count) => {
                    if count > 0 {
                        info!("Expired {} snapshot(s)", count);
                    } else {
                        debug!("SnapshotExpirationWorker: no expired snapshots");
                    }
                }
                Err(err) => {
                    error!("SnapshotExpirationWorker failed: {err}");
                }
            }
        }
    }
}
