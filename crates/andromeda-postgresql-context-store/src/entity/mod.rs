//! Persistence layer for NGSI-LD entities.

/// Binary COPY path for bulk entity inserts.
pub mod copy;
/// The clause 5.7.3 type and attribute discovery aggregations.
pub mod discovery;
/// Removing entities and attribute instances whose `expiresAt` has passed.
pub mod expiry;
/// Row-to-domain conversions for the `entities` table.
pub mod mapper;
/// The parallel bind vectors every entity statement is driven from.
pub mod params;
/// Direct implementations of entity policy ports.
mod port;
/// Deciding what to ask PostgreSQL for, and what to hand back unanswered.
pub mod pushdown;
/// The statements that read the `entities` table.
pub mod read;
/// The PostgreSQL entity store.
pub mod repo;
/// The statements that write the `entities` table.
pub mod write;
