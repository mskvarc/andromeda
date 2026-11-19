//! Whether a local read of what a query matches holds the entities to the
//! filters split entities leave to the gathered Entity (clauses 5.7.2.4,
//! 5.14.4.4 and 5.14.5.4).

/// Whether a local read of what a query matches holds the entities to the
/// filters split entities leave to the gathered Entity.
///
/// Clause 5.7.2.4 runs the local query of a split request over every Entity
/// found locally whose identifier, id pattern and type match, and clauses
/// 5.14.4.4 and 5.14.5.4 create an `EntityMap` under split entities from the
/// same Entities, the temporal one also requiring instances that meet the
/// temporal query. A locally held part of a split Entity may fail the other
/// filters while the Entity gathered from every Context Source meets them, so
/// those filters are left to the gathered Entity, or to the requests that
/// later run under the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFilters {
    /// Every filter of the query holds the entities read.
    Applied,
    /// The filters the split branch of the clause does not name are left out
    /// of the read.
    Deferred,
}
