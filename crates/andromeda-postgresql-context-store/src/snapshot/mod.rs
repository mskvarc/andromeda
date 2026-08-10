//! Persistence layer for snapshot records.

/// Row-to-domain conversions for snapshot records.
pub mod mapper;
/// Statement-shaped parameters for snapshot queries.
pub mod params;
/// The snapshot repository struct.
pub mod repo;
/// Placeholder for persistence-shaped snapshot row structs.
pub mod rows;
pub mod store;
