//! Persistence layer for NGSI-LD entities.

/// Binary COPY path for bulk entity inserts.
pub mod copy;
/// The clauses 5.7.5-5.7.10 type and attribute discovery aggregations.
pub mod discovery;
/// Removing entities and attribute instances whose `expiresAt` has passed.
pub mod expiry;
/// Resolving surrogate keys of entities a commit touched without writing them.
pub mod keys;
/// Row-to-domain conversions for the `entities` table.
pub mod mapper;
/// The parallel bind vectors every entity statement is driven from.
pub mod params;
/// Direct implementations of entity policy ports.
mod port;
/// Deciding what to ask PostgreSQL for, and what to hand back unanswered.
pub mod pushdown;
/// The statements that read one entity's current state.
pub mod read;
/// The PostgreSQL entity store.
pub mod repo;
/// The clause 4.5.1 invariant that every stored entity carries at least one type.
pub mod types_present;
/// The statements that write the three current-state entity relations.
pub mod write;
/// The identity a write statement hands back for each entity row it affected.
pub mod written_entity;
