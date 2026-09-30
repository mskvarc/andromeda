//! Shared subscription persistence layer.
//!
//! - [`rows`] — `SubscriptionRow` persistence struct.
//! - [`params`] — borrowed input structs.
//! - [`mapper`] — PgRow → row → domain conversion.
//! - [`stored_subscription_error`] — a stored row that does not read back.
//! - [`stored_kind`] — which family a statement addresses.
//! - [`status_code`] — the number a lifecycle state is stored as.
//! - [`interned_attributes`] — the dictionary keys of the attribute lists.
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
/// The dictionary keys a subscription's attribute lists are stored as.
mod interned_attributes;
/// PgRow-to-domain conversion for subscriptions.
pub mod mapper;
/// Borrowed input structs for subscription operations.
pub mod params;
/// Which periodic subscriptions are owed a notification.
pub(crate) mod periodic_due;
/// Direct PostgreSQL implementations of the subscription persistence ports.
mod postgresql;
/// The read-side projection every subscription query shares.
mod projection;
/// Row structures for subscriptions.
pub mod rows;
/// The number a subscription's lifecycle state is stored as.
pub(crate) mod status_code;
/// Which subscription family a statement addresses.
mod stored_kind;
/// What a subscription row that no longer reads back as a subscription looks like.
pub mod stored_subscription_error;
/// The number a subscription's `notificationTrigger` list stores each trigger
/// condition as.
pub(crate) mod trigger_code;
