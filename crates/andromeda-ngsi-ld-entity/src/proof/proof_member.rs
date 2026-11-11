//! The members ETSI TS 104 179 V1.1.1 clause 5.1.2 has every W3C Data
//! Integrity proof contain beside its `type`.

use andromeda_ngsi_ld_vocabulary::data_integrity_vocabulary::{compact::properties as compact_properties, expanded::properties};
use derive_more::Display;
use iri_rs::Iri;

/// One required member of a proof, displayed as its W3C Data Integrity v2
/// term.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum ProofMember {
    /// `created`: when the proof was made.
    #[display("{}", compact_properties::CREATED)]
    Created,
    /// `verificationMethod`: the method that verifies the proof.
    #[display("{}", compact_properties::VERIFICATION_METHOD)]
    VerificationMethod,
    /// `cryptosuite`: the suite the proof was made with.
    #[display("{}", compact_properties::CRYPTOSUITE)]
    Cryptosuite,
    /// `proofPurpose`: what the proof was made for.
    #[display("{}", compact_properties::PROOF_PURPOSE)]
    ProofPurpose,
    /// `proofValue`: the encoded signature.
    #[display("{}", compact_properties::PROOF_VALUE)]
    ProofValue,
}

impl ProofMember {
    /// The IRI the member expands to.
    #[must_use]
    pub const fn iri(self) -> Iri<&'static str> {
        match self {
            ProofMember::Created => properties::CREATED,
            ProofMember::VerificationMethod => properties::VERIFICATION_METHOD,
            ProofMember::Cryptosuite => properties::CRYPTOSUITE,
            ProofMember::ProofPurpose => properties::PROOF_PURPOSE,
            ProofMember::ProofValue => properties::PROOF_VALUE,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::proof::proof_member::ProofMember;

    #[test]
    fn each_member_is_displayed_as_its_term_and_expands_to_its_iri() {
        for (member, term, iri) in [
            (ProofMember::Created, "created", "http://purl.org/dc/terms/created"),
            (
                ProofMember::VerificationMethod,
                "verificationMethod",
                "https://w3id.org/security#verificationMethod",
            ),
            (ProofMember::Cryptosuite, "cryptosuite", "https://w3id.org/security#cryptosuite"),
            (ProofMember::ProofPurpose, "proofPurpose", "https://w3id.org/security#proofPurpose"),
            (ProofMember::ProofValue, "proofValue", "https://w3id.org/security#proofValue"),
        ] {
            pretty_assertions::assert_eq!((member.to_string().as_str(), member.iri().as_str()), (term, iri));
        }
    }
}
