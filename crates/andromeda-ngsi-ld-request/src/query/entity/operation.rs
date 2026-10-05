//! The operations an entity query parameter set is read for.

use andromeda_linked_retrieval::admission::LinkedOperation;

/// The operation an [`EntityQueryParameters`] set is validated for.
///
/// One parameter set serves Query Entities, the Entity Map it creates and
/// Purge Entities, and the three admit Linked Entity references differently
/// (clauses 5.7.2.4, 5.14.4.4 and 5.6.21.4).
///
/// [`EntityQueryParameters`]: crate::query::entity::parameters::EntityQueryParameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityQueryOperation {
    /// Query Entities (clause 5.7.2).
    Query,
    /// Entity Map creation for Query Entities (clause 5.14.4).
    EntityMap,
    /// Purge Entities (clause 5.6.21).
    Purge,
}

impl EntityQueryOperation {
    /// The operation as Linked Entity admission names it.
    #[must_use]
    pub const fn linked(self) -> LinkedOperation {
        match self {
            EntityQueryOperation::Query => LinkedOperation::QueryEntities,
            EntityQueryOperation::EntityMap => LinkedOperation::EntityMapQuery,
            EntityQueryOperation::Purge => LinkedOperation::PurgeEntities,
        }
    }
}
