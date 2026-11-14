//! The page a store answered a compiled entity query with.

use crate::port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry};

/// The page of matching entities a store read for a compiled query, with the
/// clause 6.3.13 count when the query asked for one.
#[derive(Debug)]
pub struct QueryOutcome {
    /// The stored states of the page's matching entities, each with its
    /// entity-level system times, or its identifier in its place when it does
    /// not read back.
    pub rows: Vec<StoredEntry<RetrievedEntity>>,
    /// The total number of matching entities, present when the query asked
    /// to be counted.
    pub total_count: Option<i64>,
}
