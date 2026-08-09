//! Shared subscription persistence layer.
//!
//! - [`rows`] — `SubscriptionRow` persistence struct.
//! - [`params`] — borrowed input structs.
//! - [`mapper`] — PgRow → row → domain conversion.

/// PgRow-to-domain conversion for subscriptions.
pub mod mapper;
/// Borrowed input structs for subscription operations.
pub mod params;
/// Direct PostgreSQL implementations of the subscription persistence ports.
mod postgresql;
/// Row structures for subscriptions.
pub mod rows;
