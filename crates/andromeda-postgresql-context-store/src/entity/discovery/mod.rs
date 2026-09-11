//! The aggregations behind entity type and attribute discovery (clauses 5.7.5-5.7.10).
//!
//! Each statement aggregates across every entity's search and document rows,
//! so all of them are keyset-paginated on their aggregate key. Query-plan assumption: the
//! server materialises the full aggregation before applying the keyset filter
//! and `LIMIT`, so the pagination caps transport volume rather than server-side
//! work. It exists to bound the response, not to make the aggregate cheaper.

/// The `entity_search`/`entity_documents` source and the keyset guard every
/// discovery aggregate is built on.
pub mod aggregate_source;
/// Clauses 5.7.8-5.7.10: the attribute summary and the single-attribute lookup.
pub mod attribute_discovery;
/// Reading whole entity payloads in bulk, by type or across the space.
pub mod entity_listing;
/// Clauses 5.7.5-5.7.7: the types in use and their attribute keys.
pub mod entity_type_discovery;
/// The `entity_type_names` dictionary as the type aggregates read it.
pub mod type_dictionary;
