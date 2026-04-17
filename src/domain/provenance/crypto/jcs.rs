//! JCS (RFC 8785) canonicalisation helpers shared by the `*-jcs-*`
//! cryptosuites.

use crate::domain::provenance::{atomic_entity::AtomicEntity, error::ProvenanceError, proof::DataIntegrityProof};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// SHA-256 output width in bytes.
pub const SHA256_LEN: usize = 32;

/// JCS-canonicalise an arbitrary [`Value`] and hash the bytes with SHA-256.
pub fn jcs_sha256(value: &Value) -> Result<[u8; SHA256_LEN], ProvenanceError> {
    let canonical = serde_jcs::to_vec(value).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    Ok(sha256(&canonical))
}

/// Compute the `hash_data` input required by the W3C Data Integrity spec:
/// JCS(unsigned atomic entity) -> SHA-256.
pub fn hash_unsigned_atomic_entity(entity: &AtomicEntity) -> Result<[u8; SHA256_LEN], ProvenanceError> {
    let map = entity.to_unsigned_map()?;
    let value = serde_json::to_value(&map).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    jcs_sha256(&value)
}

/// Compute the `hash_proof_config` input required by the W3C Data
/// Integrity spec: JCS(proof-options) -> SHA-256. The `proofValue` member
/// is stripped from the proof config before hashing, per the verification
/// algorithm in the suite specifications.
pub fn hash_proof_config(proof: &DataIntegrityProof) -> Result<[u8; SHA256_LEN], ProvenanceError> {
    let mut value = serde_json::to_value(proof).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    if let Value::Object(map) = &mut value {
        map.remove("proofValue");
    }
    jcs_sha256(&value)
}

/// Produce the signing input bytes as `hash_proof_config || hash_data`.
pub fn signing_input(entity: &AtomicEntity, proof: &DataIntegrityProof) -> Result<Vec<u8>, ProvenanceError> {
    let hash_proof = hash_proof_config(proof)?;
    let hash_data = hash_unsigned_atomic_entity(entity)?;
    let mut buffer = Vec::with_capacity(SHA256_LEN * 2);
    buffer.extend_from_slice(&hash_proof);
    buffer.extend_from_slice(&hash_data);
    Ok(buffer)
}

fn sha256(bytes: &[u8]) -> [u8; SHA256_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut out = [0_u8; SHA256_LEN];
    out.copy_from_slice(&result);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn jcs_sha256_is_deterministic_under_key_reordering() {
        let a = serde_json::json!({ "b": 2, "a": 1, "c": 3 });
        let b = serde_json::json!({ "a": 1, "b": 2, "c": 3 });
        pretty_assertions::assert_eq!(jcs_sha256(&a).unwrap(), jcs_sha256(&b).unwrap());
    }

    #[test]
    fn hash_proof_config_strips_proof_value() {
        let with_value = serde_json::json!({
            "type": "DataIntegrityProof",
            "cryptosuite": "eddsa-jcs-2022",
            "verificationMethod": "https://example.com/keys/1",
            "proofPurpose": "assertionMethod",
            "proofValue": "z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p"
        });
        let without_value = serde_json::json!({
            "type": "DataIntegrityProof",
            "cryptosuite": "eddsa-jcs-2022",
            "verificationMethod": "https://example.com/keys/1",
            "proofPurpose": "assertionMethod"
        });
        let proof: DataIntegrityProof = serde_json::from_value(with_value).unwrap();
        let hash_with = hash_proof_config(&proof).unwrap();
        let hash_without = jcs_sha256(&without_value).unwrap();
        pretty_assertions::assert_eq!(hash_with, hash_without);
    }

    #[test]
    fn signing_input_concatenates_proof_then_data() {
        use crate::domain::provenance::atomic_entity::AtomicEntity;
        let ae: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1 },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-jcs-2022",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": "z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p"
                }
            }
        }))
        .unwrap();
        let bytes = signing_input(&ae, &ae.proof.value).unwrap();
        pretty_assertions::assert_eq!(bytes.len(), SHA256_LEN * 2);
    }
}
