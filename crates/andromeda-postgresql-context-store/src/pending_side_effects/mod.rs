//! Durable side-effect queue for opt-in async batch processing.
//!
//! Clients requesting `Prefer: respond-async` on `/entityOperations` endpoints
//! cause the broker to persist the side-effect job (temporal history writes,
//! change events) into this queue in the same transaction as the entity
//! write. The queue is drained by a background worker with at-least-once
//! semantics.

/// Wire shapes for persisted side-effect payloads.
pub mod payload;
/// Direct entity-policy side-effect queue implementation.
mod port;
/// The Postgres-backed queue repository.
pub mod repo;
/// Row shapes returned by the side-effect repository.
pub mod rows;
