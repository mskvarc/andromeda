use crate::domain::{
    provenance::{
        cryptosuite::{Cryptosuite, ProofPurpose},
        error::ProvenanceError,
    },
    uri::Uri,
};
use chrono::{DateTime, Utc};
use derive_more::{AsRef, Deref, Display};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Fixed literal that the `type` member of a DataIntegrityProof must carry.
const DATA_INTEGRITY_PROOF_TYPE: &str = "DataIntegrityProof";

/// Zero-sized marker guarding the `type` literal of a DataIntegrityProof.
///
/// Enforces CIM 019 clause 5.1.1 by serialising as the constant string
/// `"DataIntegrityProof"` and rejecting any other value on deserialisation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DataIntegrityProofType;

impl Serialize for DataIntegrityProofType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(DATA_INTEGRITY_PROOF_TYPE)
    }
}

impl<'de> Deserialize<'de> for DataIntegrityProofType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<DataIntegrityProofType, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s == DATA_INTEGRITY_PROOF_TYPE {
            Ok(DataIntegrityProofType)
        } else {
            Err(serde::de::Error::custom(format!("expected type \"{DATA_INTEGRITY_PROOF_TYPE}\", got \"{s}\"")))
        }
    }
}

/// Multibase-encoded signature literal carried by `proofValue`.
///
/// The literal must decode under the standard W3C Data Integrity
/// multibase alphabet; validation is performed on construction via
/// [`multibase::decode`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Deref, Serialize)]
#[serde(transparent)]
pub struct ProofValue(String);

impl ProofValue {
    /// Decode the multibase literal into its underlying bytes.
    pub fn decode_bytes(&self) -> Result<(multibase::Base, Vec<u8>), ProvenanceError> {
        multibase::decode(&self.0).map_err(ProvenanceError::MultibaseError)
    }
}

impl TryFrom<String> for ProofValue {
    type Error = ProvenanceError;

    fn try_from(value: String) -> Result<ProofValue, ProvenanceError> {
        multibase::decode(&value).map_err(|_| ProvenanceError::InvalidProofValue)?;
        Ok(ProofValue(value))
    }
}

impl<'de> Deserialize<'de> for ProofValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<ProofValue, D::Error> {
        let s = String::deserialize(deserializer)?;
        ProofValue::try_from(s).map_err(serde::de::Error::custom)
    }
}

/// W3C Data Integrity proof (<https://w3c.github.io/vc-data-integrity/>).
///
/// Serialisation uses camelCase and preserves the `type` member at the
/// front to match Annex B Example 3 of ETSI GS CIM 019 v1.2.1.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataIntegrityProof {
    /// Fixed literal `"DataIntegrityProof"`.
    #[serde(rename = "type")]
    pub type_: DataIntegrityProofType,
    /// Cryptosuite identifier.
    pub cryptosuite: Cryptosuite,
    /// Creation timestamp of the proof.
    #[serde(default)]
    pub created: Option<DateTime<Utc>>,
    /// URI identifying the verification method.
    pub verification_method: Uri,
    /// Purpose of the proof.
    pub proof_purpose: ProofPurpose,
    /// Multibase-encoded signature value.
    pub proof_value: ProofValue,
    /// Optional challenge string to bind the proof against replay.
    pub challenge: Option<String>,
    /// Optional expiration timestamp.
    pub expires: Option<DateTime<Utc>>,
}

impl DataIntegrityProof {
    /// Return an error if the proof carries an `expires` timestamp that
    /// has already passed relative to `now`.
    pub fn ensure_not_expired(&self, now: DateTime<Utc>) -> Result<(), ProvenanceError> {
        match self.expires {
            Some(expires) if expires <= now => Err(ProvenanceError::ProofExpired { expires }),
            Some(_) | None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::str::FromStr;

    fn example_proof_json() -> serde_json::Value {
        serde_json::json!({
            "type": "DataIntegrityProof",
            "cryptosuite": "eddsa-jcs-2022",
            "created": "2024-02-01T12:00:00Z",
            "verificationMethod": "https://example.com/keys/1",
            "proofPurpose": "assertionMethod",
            "proofValue": "z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p"
        })
    }

    #[test]
    fn data_integrity_proof_serializes_with_type_and_camel_case() {
        let proof = DataIntegrityProof {
            type_: DataIntegrityProofType,
            cryptosuite: Cryptosuite::EddsaJcs2022,
            created: None,
            verification_method: Uri::from_str("https://example.com/keys/1").unwrap(),
            proof_purpose: ProofPurpose::AssertionMethod,
            proof_value: ProofValue::try_from("z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p".to_string()).unwrap(),
            challenge: None,
            expires: None,
        };
        let value = serde_json::to_value(&proof).expect("serialize");
        pretty_assertions::assert_eq!(value["type"], "DataIntegrityProof");
        pretty_assertions::assert_eq!(value["cryptosuite"], "eddsa-jcs-2022");
        pretty_assertions::assert_eq!(value["verificationMethod"], "https://example.com/keys/1");
        pretty_assertions::assert_eq!(value["proofPurpose"], "assertionMethod");
    }

    #[test]
    fn data_integrity_proof_deserializes_annex_b_example() {
        let proof: DataIntegrityProof = serde_json::from_value(example_proof_json()).expect("deserialize");
        pretty_assertions::assert_eq!(proof.cryptosuite, Cryptosuite::EddsaJcs2022);
        pretty_assertions::assert_eq!(proof.proof_purpose, ProofPurpose::AssertionMethod);
        assert!(proof.created.is_some());
    }

    #[test]
    fn data_integrity_proof_type_rejects_non_literal_type() {
        let mut json = example_proof_json();
        json["type"] = serde_json::json!("Ed25519Signature2020");
        let parsed: Result<DataIntegrityProof, _> = serde_json::from_value(json);
        assert!(parsed.is_err());
    }

    #[test]
    fn proof_value_accepts_multibase_base58btc_z_prefix() {
        let v = ProofValue::try_from("z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p".to_string());
        assert!(v.is_ok());
    }

    #[test]
    fn proof_value_rejects_non_multibase_string() {
        let v = ProofValue::try_from("not multibase".to_string());
        assert!(v.is_err());
    }

    #[test]
    fn data_integrity_proof_skips_absent_optional_fields_on_serialize() {
        let proof: DataIntegrityProof = serde_json::from_value(example_proof_json()).unwrap();
        let mut round = DataIntegrityProof {
            challenge: None,
            expires: None,
            created: None,
            ..proof
        };
        round.created = None;
        let value = serde_json::to_value(&round).expect("serialize");
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("challenge"));
        assert!(!obj.contains_key("expires"));
        assert!(!obj.contains_key("created"));
    }

    #[test]
    fn data_integrity_proof_roundtrips_optional_fields() {
        let mut json = example_proof_json();
        json["challenge"] = serde_json::json!("nonce-123");
        json["expires"] = serde_json::json!("2099-01-01T00:00:00Z");
        let proof: DataIntegrityProof = serde_json::from_value(json.clone()).expect("deserialize");
        pretty_assertions::assert_eq!(proof.challenge.as_deref(), Some("nonce-123"));
        assert!(proof.expires.is_some());
        let roundtrip = serde_json::to_value(&proof).expect("serialize");
        pretty_assertions::assert_eq!(roundtrip["challenge"], "nonce-123");
    }
}
