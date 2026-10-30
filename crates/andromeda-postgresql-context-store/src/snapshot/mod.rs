//! Persistence layer for snapshot records.

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
/// The session a clone's copy runs in, which a deletion ends.
mod clone_copy_session;
/// Creating a Snapshot's control record, preparation row and branch schemas.
mod creation;
/// Encoding a typed value as the `json` a statement writes.
mod json_value;
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
/// A Snapshot document the store could not write as JSON.
pub mod unencodable_snapshot_error;

#[cfg(test)]
mod tests {
    use crate::store_fault::persistence_error;
    use andromeda_snapshot::error::SnapshotError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(
            persistence_error::<SnapshotError>(sqlx::Error::PoolTimedOut),
            SnapshotError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error::<SnapshotError>(sqlx::Error::RowNotFound),
            SnapshotError::Persistence { .. }
        ));
    }
}
