//! `eddsa-jcs-2022` cryptosuite verifier.

use crate::domain::provenance::{
    atomic_entity::AtomicEntity,
    crypto::{CryptoSuiteVerifier, VerificationOutcome, jcs},
    cryptosuite::Cryptosuite,
    error::ProvenanceError,
    key::VerificationKey,
    proof::{DataIntegrityProof, ProofValue},
};
use ed25519_dalek::{Signature, Verifier};

/// Verifier for the W3C `eddsa-jcs-2022` cryptosuite.
#[derive(Debug, Clone, Copy)]
pub struct EddsaJcs2022;

impl CryptoSuiteVerifier for EddsaJcs2022 {
    fn cryptosuite(&self) -> Cryptosuite {
        Cryptosuite::EddsaJcs2022
    }

    fn verify(
        &self,
        unsigned: &AtomicEntity,
        proof: &DataIntegrityProof,
        proof_value: &ProofValue,
        key: &VerificationKey,
    ) -> Result<VerificationOutcome, ProvenanceError> {
        if proof.cryptosuite != Cryptosuite::EddsaJcs2022 {
            return Err(ProvenanceError::UnsupportedCryptosuite(proof.cryptosuite));
        }
        let verifying_key = match key {
            VerificationKey::Ed25519(k) => k,
            _other => return Err(ProvenanceError::KeyError("eddsa-jcs-2022 requires an Ed25519 key".into())),
        };
        let (_, sig_bytes) = proof_value.decode_bytes()?;
        let signature_bytes: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| ProvenanceError::InvalidProofValue)?;
        let signature = Signature::from_bytes(&signature_bytes);
        let message = jcs::signing_input(unsigned, proof)?;
        match verifying_key.verify(&message, &signature) {
            Ok(()) => Ok(VerificationOutcome::Valid),
            Err(_) => Ok(VerificationOutcome::Invalid),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::domain::provenance::{
        atomic_entity::AtomicEntity,
        cryptosuite::ProofPurpose,
        ngsild_proof::{NgsildProof, NgsildProofType},
        proof::{DataIntegrityProofType, ProofValue},
    };
    use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey};
    use std::str::FromStr;

    fn fixed_signing_key() -> Ed25519SigningKey {
        Ed25519SigningKey::from_bytes(&[42u8; 32])
    }

    fn signed_atomic_entity(modify_core: bool) -> (AtomicEntity, Ed25519VerifyingKey) {
        let signing = fixed_signing_key();
        let verifying = signing.verifying_key();
        let core_value = if modify_core { 2 } else { 1 };
        let mut unsigned: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": core_value },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-jcs-2022",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": "z1111111111111111111111111111111111111111111111111111111111111111111111111111111111111"
                }
            }
        }))
        .expect("atomic entity");

        let message = jcs::signing_input(&unsigned, &unsigned.proof.value).expect("signing input");
        let signature = signing.sign(&message);
        let mut sig_bytes: Vec<u8> = Vec::with_capacity(64);
        sig_bytes.extend_from_slice(&signature.to_bytes());
        let proof_value = ProofValue::try_from(multibase::encode(multibase::Base::Base58Btc, sig_bytes)).unwrap();

        let proof = DataIntegrityProof {
            type_: DataIntegrityProofType,
            cryptosuite: Cryptosuite::EddsaJcs2022,
            created: None,
            verification_method: crate::domain::uri::Uri::from_str("https://example.com/keys/1").unwrap(),
            proof_purpose: ProofPurpose::AssertionMethod,
            proof_value: proof_value.clone(),
            challenge: None,
            expires: None,
        };
        unsigned.proof = NgsildProof {
            type_: NgsildProofType,
            value: proof,
            entity_id_sealed: None,
            entity_type_sealed: None,
        };
        (unsigned, verifying)
    }

    #[test]
    fn eddsa_jcs_verifies_valid_proof_from_known_vector() {
        let (ae, key) = signed_atomic_entity(false);
        let verifier = EddsaJcs2022;
        let outcome = verifier
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(key))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Valid);
    }

    #[test]
    fn eddsa_jcs_rejects_tampered_core() {
        let (mut ae, key) = signed_atomic_entity(false);
        if let crate::domain::attribute::Attribute::Property(p) = &mut ae.attribute {
            p.value = crate::domain::attribute::common::NgsiLdValue::Json(serde_json::json!(999));
        }
        let verifier = EddsaJcs2022;
        let outcome = verifier
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(key))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn eddsa_jcs_rejects_tampered_head_id() {
        let (mut ae, key) = signed_atomic_entity(false);
        ae.head.id = crate::domain::uri::Uri::from_str("urn:ngsi-ld:Store:999").unwrap();
        let verifier = EddsaJcs2022;
        let outcome = verifier
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(key))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn eddsa_jcs_rejects_tampered_head_type() {
        let (mut ae, key) = signed_atomic_entity(false);
        ae.head.type_ = crate::domain::entity::EntityTypeValue::Single("Tampered".into());
        let verifier = EddsaJcs2022;
        let outcome = verifier
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(key))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn eddsa_jcs_rejects_wrong_verification_key() {
        let (ae, _) = signed_atomic_entity(false);
        let other = Ed25519SigningKey::from_bytes(&[99u8; 32]).verifying_key();
        let verifier = EddsaJcs2022;
        let outcome = verifier
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(other))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn eddsa_jcs_rejects_wrong_cryptosuite_value() {
        let (mut ae, key) = signed_atomic_entity(false);
        ae.proof.value.cryptosuite = Cryptosuite::EcdsaJcs2019;
        let verifier = EddsaJcs2022;
        let outcome = verifier.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(key));
        assert!(matches!(outcome, Err(ProvenanceError::UnsupportedCryptosuite(_))));
    }

    #[test]
    fn eddsa_jcs_rejects_malformed_proof_value_length() {
        let (ae, key) = signed_atomic_entity(false);
        let short = ProofValue::try_from(multibase::encode(multibase::Base::Base58Btc, [1_u8, 2, 3])).unwrap();
        let verifier = EddsaJcs2022;
        let outcome = verifier.verify(&ae, &ae.proof.value, &short, &VerificationKey::Ed25519(key));
        assert!(matches!(outcome, Err(ProvenanceError::InvalidProofValue)));
    }
}
