//! Persistence layer for context source registrations.

/// The routing projection derived from a registration's `information`.
pub mod information_projection;
/// Row-to-domain mapping for context source registrations.
pub mod mapper;
/// The number the routing projection stores a clause 4.20 operation as.
pub mod operation_code;
/// PostgreSQL registration store.
pub mod registration_store;
/// Placeholder for persistence-shaped row structs.
pub mod rows;
