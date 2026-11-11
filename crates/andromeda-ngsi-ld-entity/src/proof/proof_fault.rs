//! Why an `ngsildproof` is not the seal ETSI GS CIM 009 v1.9.1 clause 4.5.2.2
//! and ETSI TS 104 179 V1.1.1 clauses 5.1.2 and 5.2.3 define.

use crate::{member_role::ProofSeal, proof::proof_member::ProofMember};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use thiserror::Error;

/// What an `ngsildproof` lacks or holds that a seal may not.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProofFault {
    /// The `ngsildproof` is another family than a Property.
    #[error("it is a {0}, and a seal is a Property")]
    NotAProperty(AttributeType),
    /// The `value` is not one JSON object.
    #[error("its value is not one JSON object")]
    NotOneObject,
    /// The proof's `type` is not the fixed `DataIntegrityProof`.
    #[error("its value is not a proof of type DataIntegrityProof")]
    NotDataIntegrityProof,
    /// The proof lacks a required member.
    #[error("its proof carries no {0}")]
    MissingMember(ProofMember),
    /// A required member is not one value of the type it has.
    #[error("its proof member {0} is not one value of the type the W3C Data Integrity context gives it")]
    MalformedMember(ProofMember),
    /// The `ngsildproof` lacks a seal.
    #[error("it carries no {0}")]
    MissingSeal(ProofSeal),
    /// A seal is not the identifier or the entity types of an entity.
    #[error("its {0} does not name an entity identifier or entity types")]
    MalformedSeal(ProofSeal),
}
