//! Persistence layer for context source registrations.

/// The routing projection derived from a registration's `information`.
pub mod information_projection;
/// Row-to-domain mapping for context source registrations.
pub mod mapper;
/// The number the routing projection stores a clause 4.20 operation as.
pub mod operation_code;
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
/// Placeholder for persistence-shaped row structs.
pub mod rows;
