//! The operations a temporal query parameter set is read for.

use andromeda_linked_retrieval::admission::{LinkedOperation, TemporalJoin};

/// The operation a [`TemporalQueryParameters`] set is validated for.
///
/// Retrieve Temporal Evolution takes its window optionally (clause 5.7.3.3),
/// while Query Temporal Evolution and the Entity Map it creates require one
/// (clauses 5.7.4.4 and 5.14.5.4); all three admit Linked Entity references
/// by the same rule, which the `temporal_linked_retrieval` extension changes.
///
/// [`TemporalQueryParameters`]: crate::query::temporal::TemporalQueryParameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalQueryOperation {
    /// Retrieve Temporal Evolution of an Entity (clause 5.7.3).
    Retrieve,
    /// Query Temporal Evolution of Entities (clause 5.7.4).
    Query,
    /// Entity Map creation for Query Temporal Evolution (clause 5.14.5).
    EntityMap,
}

impl TemporalQueryOperation {
    /// The operation as Linked Entity admission names it, under `join`.
    #[must_use]
    pub const fn linked(self, join: TemporalJoin) -> LinkedOperation {
        match self {
            TemporalQueryOperation::Retrieve => LinkedOperation::RetrieveTemporal(join),
            TemporalQueryOperation::Query => LinkedOperation::QueryTemporal(join),
            TemporalQueryOperation::EntityMap => LinkedOperation::EntityMapTemporalQuery(join),
        }
    }
}
