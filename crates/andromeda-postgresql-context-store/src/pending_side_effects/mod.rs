//! Durable side-effect queue for opt-in async batch processing.
//!
//! Clients requesting `Prefer: respond-async` on `/entityOperations` endpoints
//! cause the broker to persist, in the same transaction as the entity write,
//! the changes whose notifications that write did not record as durable
//! deliveries. The write's temporal history is queued as temporal append
//! intents beside it, not here. The queue is drained by a background worker
//! with at-least-once semantics.

/// One owed change as the `pending_change_event` composite, and back.
pub mod change_event_record;
/// One claimed job read back from its row.
mod claimed_job;
/// The numbers an owed change's origin is stored as.
pub(crate) mod origin_code;
/// Direct entity-policy side-effect queue implementation.
mod port;
/// The Postgres-backed queue repository.
pub mod repo;
