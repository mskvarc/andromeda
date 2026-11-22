//! Shared subscription persistence layer.
//!
//! - [`rows`]: `SubscriptionRow` persistence struct.
//! - [`mapper`]: PgRow → row → domain conversion.
//! - [`member_reading`]: the members read in the expanded stage.
//! - [`stored_subscription_error`]: a stored row that does not read back.
//! - [`stored_kind`]: which family a statement addresses.
//! - [`status_code`]: the number a lifecycle state is stored as.
//! - [`interned_attributes`]: the dictionary keys of the attribute lists.
//! - [`entity_statements`]: the entity create and replace statements and binders.
//! - [`csource_statements`]: the context-source create and replace statements and binders.
//! - [`conformance_text`]: the text `ngsildConformance` is stored as.
//! - [`dataset_text`]: the text `datasetId` selectors are stored as.
//! - [`delivery_state`]: the delivery counters and their statements.
//! - [`slot_claim`]: the claim of one notification slot, gated by its windows.
//! - [`projection`]: the read-side projection every subscription query shares.
//! - [`failure`]: how adapter failures surface at the policy boundary.
//! - [`surrogate_key`]: the surrogate key of a subscription named by its URI.

/// The fields of a subscription carrying nothing optional.
#[cfg(test)]
mod bare_fields_fixture;
/// The text a subscription's `ngsildConformance` is stored as.
mod conformance_text;
/// The create and replace statements of a context-source subscription row.
mod csource_statements;
/// The text a subscription's `datasetId` selectors are stored as.
mod dataset_text;
/// The delivery counters and the statements that read and move them.
pub(crate) mod delivery_state;
/// The subscription, branch and instants the delivery state statement tests
/// address.
#[cfg(test)]
mod delivery_state_fixture;
/// The create and replace statements of an entity subscription row.
mod entity_statements;
/// How adapter failures surface at the policy boundary.
mod failure;
/// The dictionary keys a subscription's attribute lists are stored as.
mod interned_attributes;
/// A subscription document as the `json` column value a statement writes.
mod json_document;
/// PgRow-to-domain conversion for subscriptions.
pub mod mapper;
/// The members of a stored subscription that are read in the expanded stage.
mod member_reading;
/// The lists of a stored notification told apart as expanded or written.
mod notification_lists;
/// The `active_since` and `selected_since` values a subscription write stores.
mod notifying_columns;
/// Which periodic subscriptions are owed a notification.
pub(crate) mod periodic_due;
/// Direct PostgreSQL implementations of the subscription persistence ports.
mod postgresql;
/// The read-side projection every subscription query shares.
mod projection;
/// Row structures for subscriptions.
pub mod rows;
/// The claim of one notification slot on a subscription's delivery state row.
mod slot_claim;
/// The number a subscription's lifecycle state is stored as.
pub(crate) mod status_code;
/// Which subscription family a statement addresses.
pub(crate) mod stored_kind;
/// What a subscription row that no longer reads back as a subscription looks like.
pub mod stored_subscription_error;
/// The surrogate key of a subscription named by its URI.
pub(crate) mod surrogate_key;
/// The number a subscription's `notificationTrigger` list stores each trigger
/// condition as.
pub(crate) mod trigger_code;
/// How a client write of a subscription ends its transaction.
mod write_commit;
