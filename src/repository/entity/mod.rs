//! Persistence layer for NGSI-LD entities.

pub mod copy;
/// The clause 5.7.3 type and attribute discovery aggregations.
pub mod discovery;
/// Removing entities and attribute instances whose `expiresAt` has passed.
pub mod expiry;
pub mod mapper;
pub mod params;
/// Deciding what to ask PostgreSQL for, and what to hand back unanswered.
pub mod pushdown;
/// The statements that read the `entities` table.
pub mod read;
pub mod repo;
/// The statements that write the `entities` table.
pub mod write;
