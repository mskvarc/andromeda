//! Whether a listing of what a temporal query matches holds the entities to
//! the query's value, geo and scope filters (clause 5.14.5.4).

/// Whether a listing of the identifiers a temporal query matches holds the
/// entities to the query's value, geo and scope filters.
///
/// Clause 5.14.5.4 creates an `EntityMap` under split entities from every
/// Entity found locally whose identifier, id pattern and type match and whose
/// instances meet the temporal query. A locally held part of a split Entity may
/// fail the value filter, the geoquery or the scope query while the Entity
/// gathered from every Context Source meets them, so those filters are left to
/// the requests that later run under the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFilters {
    /// Every filter of the query holds the listed entities.
    Applied,
    /// The value, geo and scope filters are left out of the listing.
    Deferred,
}
