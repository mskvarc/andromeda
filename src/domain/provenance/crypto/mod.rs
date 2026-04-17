//! W3C Data Integrity verification backend for NGSI-LD sealed attributes.
//!
//! This module implements only the verifier side of the four cryptosuites
//! required by ETSI GS CIM 019 v1.2.1 clause 5.1.1. No signing logic is
//! present; by INT-4 an intermediary NGSI-LD broker must never re-sign.

pub mod ecdsa_jcs;
pub mod ecdsa_rdfc;
pub mod eddsa_jcs;
pub mod eddsa_rdfc;
pub mod jcs;
pub mod rdfc;

use crate::domain::provenance::{
    atomic_entity::AtomicEntity,
    cryptosuite::Cryptosuite,
    error::ProvenanceError,
    key::VerificationKey,
    proof::{DataIntegrityProof, ProofValue},
};

/// Outcome of a cryptosuite verification step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// The signature matched the canonicalised input under the given key.
    Valid,
    /// The signature did not match the input.
    Invalid,
}

/// Shared verifier contract implemented by each cryptosuite backend.
///
/// Implementations consume the unsigned atomic entity (head + core,
/// tail stripped) and the already-decoded proof materials. They must
/// not touch the wire form directly beyond what the cryptosuite
/// specification mandates.
pub trait CryptoSuiteVerifier {
    /// The cryptosuite this verifier speaks. Used by dispatch logging
    /// and by cross-validation against `proof.cryptosuite`.
    fn cryptosuite(&self) -> Cryptosuite;

    /// Verify the signature carried by `proof` against the canonicalised
    /// form of `unsigned`.
    fn verify(
        &self,
        unsigned: &AtomicEntity,
        proof: &DataIntegrityProof,
        proof_value: &ProofValue,
        key: &VerificationKey,
    ) -> Result<VerificationOutcome, ProvenanceError>;
}

/// Resolve the verifier for a given cryptosuite.
pub fn for_cryptosuite(suite: Cryptosuite) -> Box<dyn CryptoSuiteVerifier + Send + Sync> {
    match suite {
        Cryptosuite::EddsaJcs2022 => Box::new(eddsa_jcs::EddsaJcs2022),
        Cryptosuite::EddsaRdfc2022 => Box::new(eddsa_rdfc::EddsaRdfc2022),
        Cryptosuite::EcdsaJcs2019 => Box::new(ecdsa_jcs::EcdsaJcs2019),
        Cryptosuite::EcdsaRdfc2019 => Box::new(ecdsa_rdfc::EcdsaRdfc2019),
    }
}
