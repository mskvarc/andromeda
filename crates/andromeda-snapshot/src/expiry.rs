//! Bounded snapshot expiry policy.

use crate::{error::SnapshotError, store::SnapshotStore};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Independently scheduled deletion of expired snapshots.
pub struct SnapshotExpiry {
    /// Deletes the Snapshots whose expiry has passed.
    store: Arc<dyn SnapshotStore>,
}

impl SnapshotExpiry {
    /// Construct expiry over the snapshot persistence port.
    pub const fn new(store: Arc<dyn SnapshotStore>) -> SnapshotExpiry {
        SnapshotExpiry { store }
    }

    /// Delete snapshots due at the explicit execution cutoff.
    ///
    /// # Errors
    ///
    /// Returns the [`SnapshotError`] reported by the store when the bounded
    /// deletion does not complete.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
        self.store.delete_expired(cutoff).await
    }
}
