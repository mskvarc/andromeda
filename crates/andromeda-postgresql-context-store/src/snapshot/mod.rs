//! Persistence layer for snapshot records.

use andromeda_snapshot::error::SnapshotError;
use std::fmt::Display;

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
fn persistence_error(error: impl Display) -> SnapshotError {
    SnapshotError::persistence(error.to_string())
}
