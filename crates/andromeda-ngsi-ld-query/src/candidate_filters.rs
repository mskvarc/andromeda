//! Whether a listing of what a query matches holds the entities to the filters
//! split entities leave to the gathered Entity (clauses 5.14.4.4 and 5.14.5.4).

/// Whether a listing of the identifiers a query matches holds the entities to
/// the filters split entities leave to the gathered Entity.
///
/// Clauses 5.14.4.4 and 5.14.5.4 create an `EntityMap` under split entities
/// from every Entity found locally whose identifier, id pattern and type match,
/// the temporal one also requiring instances that meet the temporal query. A
/// locally held part of a split Entity may fail the other filters while the
/// Entity gathered from every Context Source meets them, so those filters are
/// left to the requests that later run under the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFilters {
    /// Every filter of the query holds the listed entities.
    Applied,
    /// The filters the split branch of the creating clause does not name are
    /// left out of the listing.
    Deferred,
}
