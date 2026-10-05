//! One Relationship instance naming one target (clause 4.5.23).
//!
//! Every decision the engine takes about Linked Entities -- which are read,
//! which a projection still reaches, under which Relationship a copy is nested
//! -- reads the same unit: the entity an edge leaves, the attribute that names
//! the target, and the target. A representation reports its nodes' edges once,
//! and nothing after that walks the spelling of a Relationship again.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};

/// Whether a target may be asked of the remainder of the distributed system
/// when the local store does not hold it (clause 4.5.23.1).
///
/// Only a Relationship annotated with an `objectType` declares its object an
/// Internal Linked Entity that a Context Source may hold; a target the local
/// store holds is joined either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteEligibility {
    /// No naming Relationship carried an `objectType`: only the local store
    /// answers for the target.
    LocalOnly,
    /// A naming Relationship carried an `objectType`.
    Eligible,
}

impl RemoteEligibility {
    /// The eligibility of a target two edges name: one annotated edge is
    /// enough to make it eligible.
    #[must_use]
    pub const fn either(self, other: RemoteEligibility) -> RemoteEligibility {
        match (self, other) {
            (RemoteEligibility::Eligible, RemoteEligibility::Eligible | RemoteEligibility::LocalOnly)
            | (RemoteEligibility::LocalOnly, RemoteEligibility::Eligible) => RemoteEligibility::Eligible,
            (RemoteEligibility::LocalOnly, RemoteEligibility::LocalOnly) => RemoteEligibility::LocalOnly,
        }
    }
}

/// One target named by one Relationship or `ListRelationship` instance of one
/// entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedEdge {
    /// The entity carrying the Relationship.
    pub from: Identifier,
    /// The attribute member the Relationship is keyed by on that entity.
    pub attribute: AttributeName,
    /// The entity the Relationship names.
    pub target: Identifier,
    /// Whether the target may be fetched from outside the local store.
    pub eligibility: RemoteEligibility,
}

#[cfg(test)]
mod tests {
    use crate::edge::RemoteEligibility;
    use pretty_assertions::assert_eq;

    /// Clause 4.5.23.1: one annotated Relationship among several naming one
    /// target makes the target an Internal Linked Entity.
    #[test]
    fn one_annotated_edge_makes_a_target_eligible() {
        assert_eq!(RemoteEligibility::LocalOnly.either(RemoteEligibility::Eligible), RemoteEligibility::Eligible);
        assert_eq!(RemoteEligibility::Eligible.either(RemoteEligibility::LocalOnly), RemoteEligibility::Eligible);
        assert_eq!(RemoteEligibility::LocalOnly.either(RemoteEligibility::LocalOnly), RemoteEligibility::LocalOnly);
    }
}
