//! What an `orderBy` term sorts on once its value path is expanded (clause 4.23).

use crate::{filter::value_path::ValuePath, name_stage::Expanded};

/// What an `orderBy` term sorts on (clause 4.23).
///
/// `id`, `type`, `scope`, `createdAt` and `modifiedAt` name structural members
/// of the entity representation, not attributes, so they have no IRI to expand
/// to and they are read from their own columns rather than from the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandedOrderingTarget {
    /// The entity identifier.
    Id,
    /// The entity types.
    Type,
    /// The entity scopes.
    Scope,
    /// The instant the entity was created (clause 4.8 `createdAt`).
    CreatedAt,
    /// The instant the entity's payload was last modified (clause 4.8
    /// `modifiedAt`).
    ModifiedAt,
    /// One attribute's value, addressed by a path whose every name is an expanded IRI (clause 5.2.4).
    Attribute(ValuePath<Expanded>),
}
