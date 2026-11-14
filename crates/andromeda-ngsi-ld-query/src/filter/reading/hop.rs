//! One `rel{...}` step of a Query Term.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

/// One clause 4.9 `LinkedEntityRelation` step: the Relationship crossed into
/// the Entities it names, and the types such an Entity must carry one of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    /// The Relationship the hop crosses, expanded.
    pub(super) relationship: AttributeName,
    /// The expanded types an Entity the hop reaches must carry one of, empty
    /// when the term names none.
    pub(super) guard_types: Vec<EntityTypeName>,
}

impl Hop {
    /// The Relationship the hop crosses.
    #[must_use]
    pub const fn relationship(&self) -> &AttributeName {
        &self.relationship
    }

    /// The types an Entity the hop reaches must carry one of, empty when any
    /// Entity is admitted.
    #[must_use]
    pub const fn guard_types(&self) -> &[EntityTypeName] {
        self.guard_types.as_slice()
    }
}
