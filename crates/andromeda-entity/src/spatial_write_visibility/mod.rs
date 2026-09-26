//! Every write a broker process commits is visible to its next geo query.
//!
//! Clauses 4.10 and 5.7.2 define a geo query's answer by the geometries the
//! stored entities convey, so a spatial index may narrow a query only while it
//! holds every entity the relation could match. These tests drive the entity
//! service and the query service over one index, with no change consumer at
//! all, and prove that each configured consistency mode narrows a query that
//! follows a write to a candidate set that still contains the written entity.

/// The `commit-synchronous` mode: every write path updates the index before it returns.
mod commit_synchronous;
/// The query the tests ask and the index it is narrowed through.
mod geo_query;
/// The store the entity service writes to and reads from.
mod in_memory_store;
/// The store another broker process writes, which a watermark catches up from.
mod replicated_store;
/// The `store-verified` mode: no index narrows a query.
mod store_verified;
/// The `watermark` mode: the index catches up from the store before it narrows.
mod watermark;
/// The entity service the tests write through.
mod write_service;
