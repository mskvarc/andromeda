//! Persistence layer for snapshot records.

use crate::error::ContextStoreError;
use andromeda_postgresql_pool::{is_pool_saturated, is_transaction_rolled_back};
use andromeda_snapshot::error::SnapshotError;
use std::{any::Any, fmt::Display};

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
/// Row-to-domain conversions for snapshot records.
pub mod mapper;
mod metadata_change;
mod preparation_claim;
mod quota;
/// Reaping Snapshots by deletion, expiry and quota eviction.
mod reap;
/// Reading and advancing a Snapshot's control record and preparation state.
pub(crate) mod record;
/// The snapshot repository struct.
pub mod repo;
/// The number a Snapshot status is stored as.
pub mod status_code;
/// Durable Snapshot status-notification delivery queue.
pub mod status_outbox;
pub mod store;

/// Erase an adapter failure into the snapshot policy's persistence category.
///
/// A pool-acquire timeout is backpressure, not a fault, so it becomes
/// [`SnapshotError::StoreSaturated`] (a 503); a deadlock or serialization
/// rollback is transient contention the store can retry, so it becomes
/// [`SnapshotError::Contended`]; every other failure stays a persistence fault.
/// Callers hand the error in already erased behind `impl Display`, so the
/// concrete failure is recovered by downcast -- as a raw [`sqlx::Error`] from the
/// statements this module runs directly, or as a [`ContextStoreError`] from the
/// branch-schema DDL, which is where a `DROP SCHEMA` deadlock surfaces.
fn persistence_error(error: impl Display + Any) -> SnapshotError {
    let erased = &error as &dyn Any;
    if let Some(sqlx_error) = erased.downcast_ref::<sqlx::Error>() {
        if is_pool_saturated(sqlx_error) {
            return SnapshotError::StoreSaturated;
        }
        if is_transaction_rolled_back(sqlx_error) {
            return SnapshotError::Contended;
        }
    } else if let Some(store_error) = erased.downcast_ref::<ContextStoreError>() {
        if store_error.is_pool_saturation() {
            return SnapshotError::StoreSaturated;
        }
        if store_error.is_transaction_rolled_back() {
            return SnapshotError::Contended;
        }
    }
    SnapshotError::persistence(error.to_string())
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
