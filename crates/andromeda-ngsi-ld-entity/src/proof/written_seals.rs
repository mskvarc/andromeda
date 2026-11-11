//! The seals a written `ngsildproof` sets (clauses 5.5.8 and 5.5.12).
//!
//! A fragment may omit a seal and keep the one held, but a seal is never
//! deleted: an `ngsildproof` holds both seals or is no seal at all (ETSI TS
//! 104 179 V1.1.1 clause 5.2.3).

use crate::{
    entity_types::EntityTypes,
    member_role::ProofSeal,
    proof::{proof_fault::ProofFault, proof_seals::ProofSeals},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The seals one written instance sets.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WrittenSeals {
    /// `entityIdSealed`, when written.
    entity_id: Option<Identifier>,
    /// `entityTypeSealed`, when written.
    entity_types: Option<EntityTypes>,
}

impl WrittenSeals {
    /// The seals a write sets.
    #[must_use]
    pub const fn new(entity_id: Option<Identifier>, entity_types: Option<EntityTypes>) -> WrittenSeals {
        WrittenSeals { entity_id, entity_types }
    }

    /// `entityIdSealed`, when written.
    #[must_use]
    pub const fn entity_id(&self) -> Option<&Identifier> {
        self.entity_id.as_ref()
    }

    /// `entityTypeSealed`, when written.
    #[must_use]
    pub const fn entity_types(&self) -> Option<&EntityTypes> {
        self.entity_types.as_ref()
    }

    /// Whether neither seal is written.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entity_id.is_none() && self.entity_types.is_none()
    }

    /// The seals of a proof written whole, nothing being held.
    ///
    /// # Errors
    ///
    /// Returns [`ProofFault::MissingSeal`] naming the first seal not written.
    pub fn complete(&self) -> Result<ProofSeals, ProofFault> {
        let entity_id = self.entity_id.clone().ok_or(ProofFault::MissingSeal(ProofSeal::EntityId))?; // clone: the held seals own their copy of the borrowed fragment.
        let entity_types = self.entity_types.clone().ok_or(ProofFault::MissingSeal(ProofSeal::EntityType))?; // clone: the held seals own their copy of the borrowed fragment.
        Ok(ProofSeals::new(entity_id, entity_types))
    }

    /// The seals `held` has once each written seal replaces its own.
    #[must_use]
    pub fn laid_over(&self, held: &ProofSeals) -> ProofSeals {
        ProofSeals::new(
            self.entity_id.as_ref().unwrap_or_else(|| held.entity_id()).clone(), // clone: the held seals own their copy of the borrowed fragment.
            self.entity_types.as_ref().unwrap_or_else(|| held.entity_types()).clone(), // clone: the held seals own their copy of the borrowed fragment.
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_types::EntityTypes,
        member_role::ProofSeal,
        proof::{proof_fault::ProofFault, proof_seals::ProofSeals, written_seals::WrittenSeals},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("an identifier")
    }

    fn types(raw: &str) -> EntityTypes {
        EntityTypes::one(raw.parse().expect("a type name"))
    }

    #[test]
    fn a_written_seal_replaces_its_own_and_keeps_the_other() {
        let held = ProofSeals::new(id("urn:ngsi-ld:Store:1"), types("https://example.org/Store"));
        let written = WrittenSeals::new(Some(id("urn:ngsi-ld:Store:2")), None);
        pretty_assertions::assert_eq!(
            written.laid_over(&held),
            ProofSeals::new(id("urn:ngsi-ld:Store:2"), types("https://example.org/Store"))
        );
    }

    #[test]
    fn a_proof_written_whole_needs_both_seals() {
        pretty_assertions::assert_eq!(
            WrittenSeals::new(Some(id("urn:ngsi-ld:Store:2")), None).complete(),
            Err(ProofFault::MissingSeal(ProofSeal::EntityType))
        );
        pretty_assertions::assert_eq!(
            WrittenSeals::new(None, Some(types("https://example.org/Store"))).complete(),
            Err(ProofFault::MissingSeal(ProofSeal::EntityId))
        );
    }
}
