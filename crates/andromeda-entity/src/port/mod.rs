//! Caller-shaped ports consumed by entity policy.

/// Historical dataset identifiers needed by attribute deletion.
pub mod attribute_history;
/// Runtime-neutral execution of CPU-bound policy work.
pub mod blocking_executor;
/// The reads that let a deletion skip the payload it removes.
pub mod deletion_outline;
/// Entity discovery reads.
pub mod discovery;
/// The types an entity carries, read without its payload.
pub mod entity_types;
/// Entity expiry candidates.
pub mod expiry;
/// Whole entity payloads read a page at a time.
pub mod payload_pages;
/// Compiled entity queries and capability reporting.
pub mod query;
/// Entity payload and snapshot reads.
pub mod reader;
/// The Context Registry's view of possibly available types and attributes.
pub mod registry_types;
/// One entity read for a retrieval, with its clause 4.8 system times.
pub mod retrieved_entity;
/// The caller-owned ports entity policy is constructed over.
pub mod service_ports;
/// Typed spatial-index rebuild snapshots.
pub mod spatial_snapshot;
/// One Entity of a read over many, read back or named as unreadable.
pub mod stored_entry;
