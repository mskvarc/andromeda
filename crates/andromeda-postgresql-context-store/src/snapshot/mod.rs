//! Persistence layer for snapshot records.

use crate::store_fault::{StoreFault, persistence_failure};
use andromeda_snapshot::error::SnapshotError;

/// The advisory lock a reap and PostgreSQL maintenance share per Snapshot
/// branch.
pub mod branch_lock;
/// Per-Snapshot branch schema creation, promotion and teardown.
mod branch_schema;
/// Bringing every Snapshot's branch schemas up to the binary's branch version.
pub mod branch_upgrade;
/// Which Snapshots' branch schemas carry the binary's current branch version.
pub(crate) mod branch_version;
/// Cross-schema copying of a Snapshot branch into another (clone preparation).
mod clone;
mod clone_context;
/// Creating a Snapshot's control record, preparation row and branch schemas.
mod creation;
/// Encoding a typed value as the `jsonb` a statement writes.
mod jsonb_value;
/// Row-to-domain conversions for snapshot records.
pub mod mapper;
mod metadata_change;
mod preparation_claim;
mod quota;
/// Reaping Snapshots by deletion, expiry and quota eviction.
pub(crate) mod reap;
/// Reading and advancing a Snapshot's control record and preparation state.
pub(crate) mod record;
/// The snapshot repository struct.
pub mod repo;
/// The number a Snapshot status is stored as.
pub mod status_code;
/// Durable Snapshot status-notification delivery queue.
pub mod status_outbox;
pub mod store;
/// What a Snapshot control record that no longer reads back as a Snapshot looks like.
pub mod stored_snapshot_error;

/// Erase an adapter failure into the snapshot policy's persistence category.
///
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
fn persistence_error(error: impl StoreFault) -> SnapshotError {
    SnapshotError::from(persistence_failure(&error))
}

#[cfg(test)]
mod tests {
    use crate::snapshot::persistence_error;
    use andromeda_snapshot::error::SnapshotError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), SnapshotError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), SnapshotError::Persistence { .. }));
    }
}
