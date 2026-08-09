//! Persistence layer for the notification outbox.
//!
//! The SQL decode shape remains private; [`rows`] exposes only records whose
//! domain-valued columns have passed validation.

/// The outbox repository struct and inherent insert method.
pub mod repo;
/// Validated notification outbox records and their mapper errors.
pub mod rows;
