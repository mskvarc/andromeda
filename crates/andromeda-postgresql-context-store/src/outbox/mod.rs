//! Persistence layer for the notification outbox.
//!
//! - [`rows`] — `sqlx::FromRow` structs (crate-visible).

/// The outbox repository struct and inherent insert method.
pub mod repo;
/// Row structures for notification outbox entries.
pub mod rows;
