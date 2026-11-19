//! One `rel{...}` step of a Query Term.

use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, entity_type_iri::EntityTypeIri};

/// One clause 4.9 `LinkedEntityRelation` step: the Relationship crossed into
/// the Entities it names, and the types such an Entity must carry one of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Hop {
    /// The Relationship the hop crosses, expanded.
    pub(super) relationship: AttributeIri,
    /// The expanded types an Entity the hop reaches must carry one of, empty
    /// when the term names none.
    pub(super) guard_types: Vec<EntityTypeIri>,
}

impl Hop {
    /// The Relationship the hop crosses.
    #[must_use]
    pub const fn relationship(&self) -> &AttributeIri {
        &self.relationship
    }

    /// The types an Entity the hop reaches must carry one of, empty when any
    /// Entity is admitted.
    #[must_use]
    pub const fn guard_types(&self) -> &[EntityTypeIri] {
        self.guard_types.as_slice()
    }
}
