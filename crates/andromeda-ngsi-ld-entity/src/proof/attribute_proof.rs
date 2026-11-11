//! The `ngsildproof` of a sealed attribute: a Property whose value is a W3C
//! Data Integrity proof and whose two non-reified members, the seals, name the
//! entity the proof signs (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2, ETSI TS 104
//! 179 V1.1.1 clause 5.2.3).
//!
//! The proof is held typed. Beside it the seal holds the expanded node the
//! proof is, made from the typed members when the seal is made and never
//! changed apart from them: it is the one value of the Property's `hasValue`,
//! which the query languages, a merge and the renderer read as they read any
//! Property value.

use crate::{
    expanded_value::ExpandedValue,
    proof::{data_integrity_proof::DataIntegrityProof, proof_fault::ProofFault, proof_seals::ProofSeals},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use cefact_units::UnitCode;
use std::slice;

/// Whether `attribute` is the `ngsildproof` a seal is written under.
#[must_use]
pub fn names_a_seal(attribute: &AttributeName) -> bool {
    attribute.as_str() == properties::NGSILDPROOF.as_str()
}

/// One `ngsildproof` instance's seals, proof and Property members.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeProof {
    /// `entityIdSealed` and `entityTypeSealed`.
    seals: ProofSeals,
    /// The proof.
    proof: DataIntegrityProof,
    /// The proof as its expanded node.
    value: ExpandedValue,
    /// The `unitCode` clause 4.5.2.2 admits on any Property.
    unit_code: Option<UnitCode>,
    /// The `valueType` clause 4.5.2.2 admits on any Property.
    value_type: Option<NgsiLdReference>,
}

impl AttributeProof {
    /// The seal of `proof` to the entity `seals` name.
    #[must_use]
    pub fn new(seals: ProofSeals, proof: DataIntegrityProof) -> AttributeProof {
        let value = proof.to_expanded();
        AttributeProof {
            seals,
            proof,
            value,
            unit_code: None,
            value_type: None,
        }
    }

    /// The seal whose `hasValue` holds `values`.
    ///
    /// # Errors
    ///
    /// Returns [`ProofFault::NotOneObject`] unless `values` is one node, and
    /// the fault of a node that is no proof.
    pub fn try_from_values(seals: ProofSeals, values: Vec<ExpandedValue>) -> Result<AttributeProof, ProofFault> {
        let mut values = values.into_iter();
        let (Some(ExpandedValue::Node(node)), None) = (values.next(), values.next()) else {
            return Err(ProofFault::NotOneObject);
        };
        DataIntegrityProof::try_from(*node).map(|proof| AttributeProof::new(seals, proof))
    }

    /// The seals.
    #[must_use]
    pub const fn seals(&self) -> &ProofSeals {
        &self.seals
    }

    /// The proof.
    #[must_use]
    pub const fn proof(&self) -> &DataIntegrityProof {
        &self.proof
    }

    /// The values of the Property's `hasValue`: the proof's node alone.
    #[must_use]
    pub const fn values(&self) -> &[ExpandedValue] {
        slice::from_ref(&self.value)
    }

    /// The `unitCode`.
    #[must_use]
    pub const fn unit_code(&self) -> Option<UnitCode> {
        self.unit_code
    }

    /// The `valueType`.
    #[must_use]
    pub const fn value_type(&self) -> Option<&NgsiLdReference> {
        self.value_type.as_ref()
    }

    /// Replace the seals.
    pub fn set_seals(&mut self, seals: ProofSeals) {
        self.seals = seals;
    }

    /// Replace the proof by the one `values` hold.
    ///
    /// # Errors
    ///
    /// As [`AttributeProof::try_from_values`]; the seal is left unchanged.
    pub fn set_values(&mut self, values: Vec<ExpandedValue>) -> Result<(), ProofFault> {
        let replaced = AttributeProof::try_from_values(self.seals.clone(), values)?; // clone: the seals stay with the instance while the proof is replaced.
        self.proof = replaced.proof;
        self.value = replaced.value;
        Ok(())
    }

    /// Replace the `unitCode`.
    pub const fn set_unit_code(&mut self, unit_code: Option<UnitCode>) {
        self.unit_code = unit_code;
    }

    /// Replace the `valueType`.
    pub fn set_value_type(&mut self, value_type: Option<NgsiLdReference>) {
        self.value_type = value_type;
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_types::EntityTypes,
        expanded_value::ExpandedValue,
        literal::{Literal, LiteralValue},
        proof::{attribute_proof::AttributeProof, data_integrity_proof::DataIntegrityProof, proof_fault::ProofFault, proof_seals::ProofSeals},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;

    fn seals() -> ProofSeals {
        ProofSeals::new(
            "urn:ngsi-ld:Store:002".parse().expect("an identifier"),
            EntityTypes::one("https://example.org/Store".parse().expect("a type name")),
        )
    }

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a URI")
    }

    fn proof() -> DataIntegrityProof {
        DataIntegrityProof::builder()
            .created("2025-01-27T21:02:24Z".parse().expect("a DateTime"))
            .verification_method(uri("https://example.edu/issuers/565049#key"))
            .cryptosuite("eddsa-rdfc-2022".parse().expect("a suite"))
            .proof_purpose(uri("https://w3id.org/security#assertionMethod"))
            .proof_value("z3XrH3".parse().expect("a signature"))
            .build()
    }

    #[test]
    fn the_value_is_the_node_of_the_proof() {
        let sealed = AttributeProof::new(seals(), proof());
        pretty_assertions::assert_eq!(sealed.values(), [proof().to_expanded()]);
        pretty_assertions::assert_eq!(AttributeProof::try_from_values(seals(), vec![proof().to_expanded()]), Ok(sealed));
    }

    /// Clause 4.5.2.2: the value "shall be an object".
    #[test]
    fn a_value_that_is_not_one_object_is_refused() {
        let text = ExpandedValue::Literal(Literal::plain(LiteralValue::String("a proof".into())));
        for values in [Vec::new(), vec![text], vec![proof().to_expanded(), proof().to_expanded()]] {
            pretty_assertions::assert_eq!(AttributeProof::try_from_values(seals(), values), Err(ProofFault::NotOneObject));
        }
    }

    #[test]
    fn a_refused_replacement_leaves_the_proof_held() {
        let mut sealed = AttributeProof::new(seals(), proof());
        pretty_assertions::assert_eq!(sealed.set_values(Vec::new()), Err(ProofFault::NotOneObject));
        pretty_assertions::assert_eq!(sealed, AttributeProof::new(seals(), proof()));
    }
}
