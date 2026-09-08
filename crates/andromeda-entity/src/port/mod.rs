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
/// The Context Registry's view of possibly available types and attributes.
pub mod registry_types;
/// Runtime-neutral waiting between contended write attempts.
pub mod retry_delay;
/// The caller-owned ports entity policy is constructed over.
pub mod service_ports;
/// Typed spatial-index rebuild snapshots.
pub mod spatial_snapshot;
