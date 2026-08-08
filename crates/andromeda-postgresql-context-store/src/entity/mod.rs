//! Persistence layer for NGSI-LD entities.

/// Aggregated metadata for a single attribute across all entities.
pub mod attribute_summary;
/// Binary COPY path for bulk entity inserts.
pub mod copy;
/// One entity as it stood at the moment it was deleted.
pub mod deleted;
/// The clause 5.7.3 type and attribute discovery aggregations.
pub mod discovery;
/// Removing entities and attribute instances whose `expiresAt` has passed.
pub mod expiry;
/// Row-to-domain conversions for the `entities` table.
pub mod mapper;
/// Whether a store applied the result window a query asked for.
pub mod pagination_outcome;
/// The parallel bind vectors every entity statement is driven from.
pub mod params;
/// Deciding what to ask PostgreSQL for, and what to hand back unanswered.
pub mod pushdown;
/// What a store returned from a query, and what it left for the caller.
pub mod query_outcome;
/// The statements that read the `entities` table.
pub mod read;
/// The PostgreSQL entity store.
pub mod repo;
/// The predicates a store declined to apply.
pub mod residual_filter;
/// One kind of predicate a store may decline.
pub mod residual_predicate;
/// One entity's payload together with the version token.
pub mod snapshot;
/// Which predicates a store can push down, and how far.
pub mod storage_profile;
/// The statements that write the `entities` table.
pub mod write;
