//! The two data-integrity seals of the `ngsildproof` Property (clause 4.5.2.2).
//!
//! Clause 4.5.2.2 gives `ngsildproof` the "non-reified subproperties
//! `entityIdSealed` and `entityTypeSealed`": members of the instance that carry
//! a value directly rather than being attributes themselves.

use crate::expanded_value::ExpandedValue;

/// The seal members one instance carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProofSeals {
    /// The expanded values of `entityIdSealed`.
    entity_id: Option<Vec<ExpandedValue>>,
    /// The expanded values of `entityTypeSealed`.
    entity_type: Option<Vec<ExpandedValue>>,
}

impl ProofSeals {
    /// The seals from their members.
    #[must_use]
    pub const fn new(entity_id: Option<Vec<ExpandedValue>>, entity_type: Option<Vec<ExpandedValue>>) -> ProofSeals {
        ProofSeals { entity_id, entity_type }
    }

    /// The `entityIdSealed` member, when present.
    #[must_use]
    pub fn entity_id(&self) -> Option<&[ExpandedValue]> {
        self.entity_id.as_deref()
    }

    /// The `entityTypeSealed` member, when present.
    #[must_use]
    pub fn entity_type(&self) -> Option<&[ExpandedValue]> {
        self.entity_type.as_deref()
    }

    /// Whether neither seal is present.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entity_id.is_none() && self.entity_type.is_none()
    }
}
