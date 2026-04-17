//! `ecdsa-rdfc-2019` cryptosuite verifier.

use crate::domain::provenance::{
    atomic_entity::AtomicEntity,
    crypto::{CryptoSuiteVerifier, VerificationOutcome, rdfc},
    cryptosuite::Cryptosuite,
    error::ProvenanceError,
    key::VerificationKey,
    proof::{DataIntegrityProof, ProofValue},
};
use p256::ecdsa::{Signature as P256Signature, signature::Verifier};
use p384::ecdsa::Signature as P384Signature;

/// Verifier for the W3C `ecdsa-rdfc-2019` cryptosuite.
#[derive(Debug, Clone, Copy)]
pub struct EcdsaRdfc2019;

impl CryptoSuiteVerifier for EcdsaRdfc2019 {
    fn cryptosuite(&self) -> Cryptosuite {
        Cryptosuite::EcdsaRdfc2019
    }

    fn verify(
        &self,
        unsigned: &AtomicEntity,
        proof: &DataIntegrityProof,
        proof_value: &ProofValue,
        key: &VerificationKey,
    ) -> Result<VerificationOutcome, ProvenanceError> {
        if proof.cryptosuite != Cryptosuite::EcdsaRdfc2019 {
            return Err(ProvenanceError::UnsupportedCryptosuite(proof.cryptosuite));
        }
        let message = rdfc::signing_input(unsigned, proof)?;
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
            VerificationKey::Ed25519(_) => Err(ProvenanceError::KeyError("ecdsa-rdfc-2019 requires a P-256 or P-384 key".into())),
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

    fn signed_entity_p256(core_value: i32) -> (AtomicEntity, VerificationKey) {
        let bytes: [u8; 32] = [3u8; 32];
        let signing = P256SigningKey::from_bytes((&bytes).into()).unwrap();
        let verifying = *signing.verifying_key();
        let placeholder = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
        let mut ae: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": core_value },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "ecdsa-rdfc-2019",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": placeholder
                }
            }
        }))
        .unwrap();
        let message = rdfc::signing_input(&ae, &ae.proof.value).unwrap();
        let signature: P256Signature = signing.sign(&message);
        let proof_value = ProofValue::try_from(multibase::encode(multibase::Base::Base58Btc, signature.to_bytes().to_vec())).unwrap();
        ae.proof = NgsildProof {
            type_: NgsildProofType,
            value: DataIntegrityProof {
                type_: DataIntegrityProofType,
                cryptosuite: Cryptosuite::EcdsaRdfc2019,
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
    fn ecdsa_rdfc_verifies_valid_proof_from_known_vector() {
        let (ae, key) = signed_entity_p256(1);
        let outcome = EcdsaRdfc2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Valid);
    }

    #[test]
    fn ecdsa_rdfc_rejects_tampered_core() {
        let (mut ae, key) = signed_entity_p256(1);
        if let crate::domain::attribute::Attribute::Property(p) = &mut ae.attribute {
            p.value = crate::domain::attribute::common::NgsiLdValue::Json(serde_json::json!(999));
        }
        let outcome = EcdsaRdfc2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_rdfc_rejects_tampered_head_id() {
        let (mut ae, key) = signed_entity_p256(1);
        ae.head.id = Uri::from_str("urn:ngsi-ld:Store:999").unwrap();
        let outcome = EcdsaRdfc2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key).expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_rdfc_rejects_wrong_verification_key() {
        let (ae, _) = signed_entity_p256(1);
        let other_bytes: [u8; 32] = [9u8; 32];
        let other = *P256SigningKey::from_bytes((&other_bytes).into()).unwrap().verifying_key();
        let outcome = EcdsaRdfc2019
            .verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::EcdsaP256(other))
            .expect("verify");
        pretty_assertions::assert_eq!(outcome, VerificationOutcome::Invalid);
    }

    #[test]
    fn ecdsa_rdfc_rejects_wrong_cryptosuite_value() {
        let (mut ae, key) = signed_entity_p256(1);
        ae.proof.value.cryptosuite = Cryptosuite::EddsaRdfc2022;
        let outcome = EcdsaRdfc2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &key);
        assert!(matches!(outcome, Err(ProvenanceError::UnsupportedCryptosuite(_))));
    }

    #[test]
    fn ecdsa_rdfc_rejects_ed25519_key() {
        let (ae, _) = signed_entity_p256(1);
        let ed_key = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]).verifying_key();
        let outcome = EcdsaRdfc2019.verify(&ae, &ae.proof.value, &ae.proof.value.proof_value, &VerificationKey::Ed25519(ed_key));
        assert!(matches!(outcome, Err(ProvenanceError::KeyError(_))));
    }
}
