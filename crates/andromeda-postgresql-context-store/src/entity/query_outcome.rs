//! What a store returned from a query, and what it left for the caller.
//!
//! A port that returns only rows says nothing about which predicates produced
//! them. That is sufficient while one backend can apply every predicate the
//! query compiler emits, and it is silently wrong the moment one cannot: a
//! store with no PostGIS would answer `georel=near` with every entity it holds.
//! Saying what was not applied is what makes the difference detectable.

use crate::entity::{pagination_outcome::PaginationOutcome, residual_filter::ResidualFilter};

/// The rows a store returned, together with what it did not do to them.
///
/// Generic in the row type because the two read methods of the entity store
/// return different shapes: expanded payloads for a full query, identifiers
/// for the clause 5.2.39 entity-map path.
#[derive(Debug)]
pub struct QueryOutcome<T> {
    /// The rows the store returned.
    pub rows: Vec<T>,
    /// The predicates the store declined to apply.
    ///
    /// Empty for a store that applied everything, which is the only case a
    /// single-backend deployment produces.
    pub unapplied: ResidualFilter,
    /// Whether the clause 5.5.6 result window was applied, and what was counted.
    pub pagination: PaginationOutcome,
}
