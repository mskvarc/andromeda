//! `ecdsa-jcs-2019` cryptosuite verifier.

use crate::domain::provenance::{
    atomic_entity::AtomicEntity,
    crypto::{CryptoSuiteVerifier, VerificationOutcome, jcs},
    cryptosuite::Cryptosuite,
    error::ProvenanceError,
    key::VerificationKey,
    proof::{DataIntegrityProof, ProofValue},
};
use p256::ecdsa::{Signature as P256Signature, signature::Verifier};
use p384::ecdsa::Signature as P384Signature;

/// Verifier for the W3C `ecdsa-jcs-2019` cryptosuite.
#[derive(Debug, Clone, Copy)]
pub struct EcdsaJcs2019;

impl CryptoSuiteVerifier for EcdsaJcs2019 {
    fn cryptosuite(&self) -> Cryptosuite {
        Cryptosuite::EcdsaJcs2019
    }

    fn verify(
        &self,
        unsigned: &AtomicEntity,
        proof: &DataIntegrityProof,
        proof_value: &ProofValue,
        key: &VerificationKey,
    ) -> Result<VerificationOutcome, ProvenanceError> {
        if proof.cryptosuite != Cryptosuite::EcdsaJcs2019 {
            return Err(ProvenanceError::UnsupportedCryptosuite(proof.cryptosuite));
        }
        let message = jcs::signing_input(unsigned, proof)?;
        let (_, sig_bytes) = proof_value.decode_bytes()?;
        match key {
            VerificationKey::EcdsaP256(k) => {
                let raw: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| ProvenanceError::InvalidProofValue)?;
                let signature = P256Signature::from_slice(&raw).map_err(|_| ProvenanceError::InvalidProofValue)?;
                match k.verify(&message, &signature) {
                    Ok(()) => Ok(VerificationOutcome::Valid),
                    Err(_) => Ok(VerificationOutcome::Invalid),
                }
            }
            VerificationKey::EcdsaP384(k) => {
                let raw: [u8; 96] = sig_bytes.as_slice().try_into().map_err(|_| ProvenanceError::InvalidProofValue)?;
                let signature = P384Signature::from_slice(&raw).map_err(|_| ProvenanceError::InvalidProofValue)?;
                match k.verify(&message, &signature) {
                    Ok(()) => Ok(VerificationOutcome::Valid),
                    Err(_) => Ok(VerificationOutcome::Invalid),
                }
            }
            VerificationKey::Ed25519(_) => Err(ProvenanceError::KeyError("ecdsa-jcs-2019 requires a P-256 or P-384 key".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::domain::{
        provenance::{
            cryptosuite::ProofPurpose,
            ngsild_proof::{NgsildProof, NgsildProofType},
            proof::{DataIntegrityProofType, ProofValue},
        },
        uri::Uri,
    };
    use p256::ecdsa::{SigningKey as P256SigningKey, signature::Signer as P256Signer};
    use std::str::FromStr;

    fn fixed_p256_signing_key() -> P256SigningKey {
        let bytes: [u8; 32] = [3u8; 32];
        P256SigningKey::from_bytes((&bytes).into()).unwrap()
    }

    fn signed_entity_p256() -> (AtomicEntity, VerificationKey) {
        let signing = fixed_p256_signing_key();
        let verifying = *signing.verifying_key();
        let mut ae: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1 },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "ecdsa-jcs-2019",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": "z1111111111111111111111111111111111111111111111111111111111111111111111111111111111111"
                }
            }
        }))
        .unwrap();
        let message = jcs::signing_input(&ae, &ae.proof.value).unwrap();
        let signature: P256Signature = signing.sign(&message);
        let sig_bytes = signature.to_bytes().to_vec();
        let proof_value = ProofValue::try_from(multibase::encode(multibase::Base::Base58Btc, sig_bytes)).unwrap();
        ae.proof = NgsildProof {
            type_: NgsildProofType,
            value: DataIntegrityProof {
                type_: DataIntegrityProofType,
                cryptosuite: Cryptosuite::EcdsaJcs2019,
                created: None,
                verification_method: Uri::from_str("https://example.com/keys/1").unwrap(),
                proof_purpose: ProofPurpose::AssertionMethod,
                proof_value,
                challenge: None,
                expires: None,
            },
            entity_id_sealed: None,
            entity_type_sealed: None,
        };
        (ae, VerificationKey::EcdsaP256(verifying))
    }

    #[test]
    fn ecdsa_jcs_verifies_valid_proof_from_known_vector() {
        let (ae, key) = signed_entity_p256();
        let outcome = EcdsaJcs2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Valid);
    }

    #[test]
    fn ecdsa_jcs_rejects_tampered_core() {
        let (mut ae, key) = signed_entity_p256();
        if let crate::domain::attribute::Attribute::Property(p) = &mut ae.attribute {
            p.value = crate::domain::attribute::common::NgsiLdValue::Json(serde_json::json!(999));
        }
        let outcome = EcdsaJcs2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_jcs_rejects_tampered_head_id() {
        let (mut ae, key) = signed_entity_p256();
        ae.head.id = Uri::from_str("urn:ngsi-ld:Store:999").unwrap();
        let outcome = EcdsaJcs2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_jcs_rejects_tampered_head_type() {
        let (mut ae, key) = signed_entity_p256();
        ae.head.type_ = crate::domain::entity::EntityTypeValue::Single("Tampered".into());
        let outcome = EcdsaJcs2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_jcs_rejects_wrong_verification_key() {
        let (ae, _) = signed_entity_p256();
        let other_bytes: [u8; 32] = [9u8; 32];
        let other = *P256SigningKey::from_bytes((&other_bytes).into()).unwrap().verifying_key();
        let outcome = EcdsaJcs2019
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::EcdsaP256(other))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_jcs_rejects_wrong_cryptosuite_value() {
        let (mut ae, key) = signed_entity_p256();
        ae.proof.value.cryptosuite = Cryptosuite::EddsaJcs2022;
        let outcome = EcdsaJcs2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key);
        assert!(matches!(outcome, Err(ProvenanceError::UnsupportedCryptosuite(_))));
    }

    #[test]
    fn ecdsa_jcs_rejects_ed25519_key() {
        let (ae, _) = signed_entity_p256();
        let ed_key = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]).verifying_key();
        let outcome = EcdsaJcs2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(ed_key));
        assert!(matches!(outcome, Err(ProvenanceError::KeyError(_))));
    }
}
