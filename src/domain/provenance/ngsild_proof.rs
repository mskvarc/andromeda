use crate::domain::{entity::EntityTypeValue, provenance::proof::DataIntegrityProof, uri::Uri};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Fixed literal that the `type` member of an NGSI-LD proof sub-property
/// must carry (ETSI GS CIM 019 v1.2.1 clause 5.2.1 - the proof is always
/// carried as a Property).
const NGSILD_PROOF_TYPE: &str = "Property";

/// Marker enforcing the `"Property"` type literal on an [`NgsildProof`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NgsildProofType;

impl Serialize for NgsildProofType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(NGSILD_PROOF_TYPE)
    }
}

impl<'de> Deserialize<'de> for NgsildProofType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<NgsildProofType, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s == NGSILD_PROOF_TYPE {
            Ok(NgsildProofType)
        } else {
            Err(serde::de::Error::custom(format!("expected type \"{NGSILD_PROOF_TYPE}\", got \"{s}\"")))
        }
    }
}

/// NGSI-LD Data Integrity proof sub-property (ETSI GS CIM 019 v1.2.1
/// clause 5.2.1). It is modelled as a Property whose `value` is a
/// [`DataIntegrityProof`] and which may carry the sealed head values
/// `entityIdSealed` and `entityTypeSealed` (clause 5.2.5).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NgsildProof {
    /// Literal `"Property"`.
    #[serde(rename = "type")]
    pub type_: NgsildProofType,
    /// W3C Data Integrity proof value.
    pub value: DataIntegrityProof,
    /// Optional preserved entity id from the point at which the proof
    /// was generated.
    pub entity_id_sealed: Option<Uri>,
    /// Optional preserved entity type from the point at which the proof
    /// was generated.
    pub entity_type_sealed: Option<EntityTypeValue>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    fn sample_json() -> serde_json::Value {
        serde_json::json!({
            "type": "Property",
            "value": {
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "verificationMethod": "https://example.com/keys/1",
                "proofPurpose": "assertionMethod",
                "proofValue": "z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p"
            },
            "entityIdSealed": "urn:ngsi-ld:Store:002",
            "entityTypeSealed": "Store"
        })
    }

    #[test]
    fn ngsild_proof_deserializes_from_compact_json() {
        let proof: NgsildProof = serde_json::from_value(sample_json()).expect("deserialize");
        assert!(proof.entity_id_sealed.is_some());
        assert!(proof.entity_type_sealed.is_some());
    }

    #[test]
    fn ngsild_proof_requires_property_type() {
        let mut json = sample_json();
        json["type"] = serde_json::json!("Relationship");
        let result: Result<NgsildProof, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn ngsild_proof_serializes_entity_id_sealed_and_entity_type_sealed() {
        let proof: NgsildProof = serde_json::from_value(sample_json()).unwrap();
        let value = serde_json::to_value(&proof).expect("serialize");
        pretty_assertions::assert_eq!(value["type"], "Property");
        pretty_assertions::assert_eq!(value["entityIdSealed"], "urn:ngsi-ld:Store:002");
        pretty_assertions::assert_eq!(value["entityTypeSealed"], "Store");
    }

    #[test]
    fn ngsild_proof_omits_sealed_fields_when_absent() {
        let mut json = sample_json();
        json.as_object_mut().unwrap().remove("entityIdSealed");
        json.as_object_mut().unwrap().remove("entityTypeSealed");
        let proof: NgsildProof = serde_json::from_value(json).unwrap();
        let value = serde_json::to_value(&proof).expect("serialize");
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("entityIdSealed"));
        assert!(!obj.contains_key("entityTypeSealed"));
    }
}
