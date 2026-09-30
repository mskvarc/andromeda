//! Persistence layer for context source registrations.

/// The portable restatement of every `idPattern` a registration declares.
mod entity_id_pattern_forms;
/// The routing projection derived from a registration's `information`.
pub mod information_projection;
/// Row-to-domain mapping for context source registrations.
pub mod mapper;
/// The number the `mode` column stores a forwarding mode as.
pub mod mode_code;
/// The number a registration stores a clause 4.20 operation as.
pub mod operation_code;
/// Rendering stored identifier-pattern restatements again after a rendering
/// change.
pub mod pattern_forms_refresh;
/// The read side of the registration store.
mod registration_read;
/// The statements the registration store builds on.
mod registration_statements;
/// PostgreSQL registration store.
pub mod registration_store;
/// The write side of the registration store.
mod registration_write;
/// The registry's possibly available types and attributes (clause 5.7.11).
pub mod registry_type_source;
/// The wholesale rewrite of a registration's routing rows.
pub mod routing_rewrite;
/// The number the delivery state stores a registration's health as.
pub mod status_code;
/// What a registration row that no longer reads back as a registration looks like.
pub mod stored_registration_error;
