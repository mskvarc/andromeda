//! Caller-shaped ports consumed by entity policy.

/// Historical dataset identifiers needed by attribute deletion.
pub mod attribute_history;
/// Runtime-neutral execution of CPU-bound policy work.
pub mod blocking_executor;
/// Entity discovery reads.
pub mod discovery;
/// Entity expiry candidates.
pub mod expiry;
/// Compiled entity queries and capability reporting.
pub mod query;
/// Entity payload and snapshot reads.
pub mod reader;
/// Runtime-neutral waiting between contended write attempts.
pub mod retry_delay;
/// Typed spatial-index rebuild snapshots.
pub mod spatial_snapshot;
