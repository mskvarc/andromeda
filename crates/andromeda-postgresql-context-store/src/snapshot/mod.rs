//! Persistence layer for snapshot records.

use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_snapshot::error::SnapshotError;
use std::{any::Any, fmt::Display};

/// Per-Snapshot branch schema creation, promotion and teardown.
mod branch_schema;
/// Cross-schema copying of a Snapshot branch into another (clone preparation).
mod clone;
mod clone_context;
/// The statements over the `snapshots` control record.
pub mod lifecycle;
/// Row-to-domain conversions for snapshot records.
pub mod mapper;
mod metadata_change;
mod preparation_claim;
mod quota;
/// The snapshot repository struct.
pub mod repo;
/// Placeholder for persistence-shaped snapshot row structs.
pub mod rows;
/// Durable Snapshot status-notification delivery queue.
pub mod status_outbox;
pub mod store;

/// Erase an adapter failure into the snapshot policy's persistence category.
///
/// A `sqlx` pool-acquire timeout is backpressure, not a fault, so it becomes
/// [`SnapshotError::StoreSaturated`] (a 503); every other failure stays a
/// persistence fault. The concrete [`sqlx::Error`] is recovered by downcast
/// because callers hand it in already erased behind `impl Display`.
fn persistence_error(error: impl Display + Any) -> SnapshotError {
    match (&error as &dyn Any).downcast_ref::<sqlx::Error>() {
        Some(sqlx_error) if is_pool_saturated(sqlx_error) => SnapshotError::StoreSaturated,
        Some(_) | None => SnapshotError::persistence(error.to_string()),
    }
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
