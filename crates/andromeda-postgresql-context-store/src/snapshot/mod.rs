//! Persistence layer for snapshot records.

use andromeda_snapshot::error::SnapshotError;
use std::fmt::Display;

mod clone_context;
/// The statements over the `snapshots` control record.
pub mod lifecycle;
/// Row-to-domain conversions for snapshot records.
pub mod mapper;
/// The snapshot repository struct.
pub mod repo;
/// Placeholder for persistence-shaped snapshot row structs.
pub mod rows;
pub mod store;

/// Erase an adapter failure into the snapshot policy's persistence category.
fn persistence_error(error: impl Display) -> SnapshotError {
    SnapshotError::persistence(error.to_string())
}
