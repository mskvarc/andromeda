//! Shared subscription persistence layer.
//!
//! - [`rows`] — `SubscriptionRow` persistence struct.
//! - [`params`] — borrowed input structs.
//! - [`mapper`] — PgRow → row → domain conversion.
//! - [`stored_kind`] — which family a statement addresses.
//! - [`create_update_sql`] — the create and replace statements and binders.
//! - [`delivery_state`] — the delivery counters and their statements.
//! - [`projection`] — the read-side projection every subscription query shares.
//! - [`failure`] — how adapter failures surface at the policy boundary.

/// The create and replace statements of the subscription row.
mod create_update_sql;
/// The delivery counters and the statements that read and move them.
pub(crate) mod delivery_state;
/// How adapter failures surface at the policy boundary.
mod failure;
/// PgRow-to-domain conversion for subscriptions.
pub mod mapper;
/// Borrowed input structs for subscription operations.
pub mod params;
/// Direct PostgreSQL implementations of the subscription persistence ports.
mod postgresql;
/// The read-side projection every subscription query shares.
mod projection;
/// Row structures for subscriptions.
pub mod rows;
/// Which subscription family a statement addresses.
mod stored_kind;
