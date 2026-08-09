//! Application services implementing NGSI-LD business logic.
//!
//! Services orchestrate domain operations, coordinate repositories, and enforce
//! business rules. They sit between the API layer and persistence, providing
//! the core functionality for entity management, subscriptions, and context
//! source registration.

/// The transactional notification outbox.
pub mod outbox;
pub mod snapshot;
pub mod snapshot_executor;
pub mod snapshot_prepare;
pub mod snapshot_status;
pub mod temporal;
