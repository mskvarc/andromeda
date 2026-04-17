use crate::domain::{
    attribute::{Attribute, AttributeValue},
    entity::{Entity, EntityTypeValue},
    provenance::{error::ProvenanceError, ngsild_proof::NgsildProof},
    uri::Uri,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, de::Error as _};
use serde_json::Value;

/// Head of an Atomic Entity (ETSI GS CIM 019 v1.2.1 clause 5.2.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicEntityHead {
    /// Entity identifier.
    pub id: Uri,
    /// Entity type value.
    #[serde(rename = "type")]
    pub type_: EntityTypeValue,
}

/// Atomic Entity used as the canonical input to a Data Integrity proof
/// (ETSI GS CIM 019 v1.2.1 clause 5.2.2). An atomic entity carries
/// exactly one attribute and exactly one `ngsildproof` tail; this
/// invariant is enforced on construction via [`AtomicEntity::from_entity`]
/// / [`TryFrom<Entity>`].
///
/// JSON serialisation preserves the order head -> core -> tail, which
/// matches both the worked example in Annex B and the ordering
/// requirement for RDFC-1.0 canonicalisation inputs.
#[derive(Debug, Clone)]
pub struct AtomicEntity {
    /// Head: entity id and type.
    pub head: AtomicEntityHead,
    /// Core attribute name.
    pub attribute_name: String,
    /// Core attribute payload.
    pub attribute: Attribute,
    /// Tail: the NGSI-LD proof sub-property.
    pub proof: NgsildProof,
}

impl AtomicEntity {
    /// Name of the tail member holding the proof.
    pub const PROOF_KEY: &'static str = "ngsildproof";

    /// Construct an atomic entity from an [`Entity`], enforcing the one
    /// attribute + one proof invariant from CIM 019 clause 5.2.2.
    pub fn from_entity(entity: &Entity) -> Result<AtomicEntity, ProvenanceError> {
        let mut core: Option<(String, Attribute)> = None;
        let mut proof: Option<NgsildProof> = None;
        let mut other_count = 0_usize;

        for (name, value) in &entity.attrs {
            if name == AtomicEntity::PROOF_KEY {
                let json = serde_json::to_value(value).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
                let parsed: NgsildProof = serde_json::from_value(attribute_value_as_single_json(&json)?)
                    .map_err(|e| ProvenanceError::InvalidNgsildProof { reason: e.to_string() })?;
                proof = Some(parsed);
                continue;
            }
            if core.is_some() {
                other_count += 1;
                continue;
            }
            let attr = attribute_value_as_single(value)?;
            core = Some((name.clone(), attr));
        }

        let proof = proof.ok_or(ProvenanceError::MissingNgsildProof)?;
        let (attribute_name, attribute) = match core {
            Some(pair) => pair,
            None => return Err(ProvenanceError::InvalidAttributeCount(0)),
        };
        if other_count > 0 {
            return Err(ProvenanceError::InvalidAttributeCount(1 + other_count));
        }

        Ok(AtomicEntity {
            head: AtomicEntityHead {
                id: entity.id.clone(),
                type_: entity.type_.clone(),
            },
            attribute_name,
            attribute,
            proof,
        })
    }

    /// Render the atomic entity as an ordered map (id, type, core, tail).
    pub fn to_ordered_map(&self) -> Result<IndexMap<String, Value>, ProvenanceError> {
        let mut map: IndexMap<String, Value> = IndexMap::new();
        map.insert(
            "id".to_string(),
            serde_json::to_value(&self.head.id).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?,
        );
        map.insert(
            "type".to_string(),
            serde_json::to_value(&self.head.type_).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?,
        );
        map.insert(
            self.attribute_name.clone(),
            serde_json::to_value(&self.attribute).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?,
        );
        map.insert(
            AtomicEntity::PROOF_KEY.to_string(),
            serde_json::to_value(&self.proof).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?,
        );
        Ok(map)
    }

    /// Render the atomic entity as an ordered map, dropping the proof tail.
    ///
    /// The unsigned form is the input to the cryptosuite hashing step.
    pub fn to_unsigned_map(&self) -> Result<IndexMap<String, Value>, ProvenanceError> {
        let mut map = self.to_ordered_map()?;
        map.shift_remove(AtomicEntity::PROOF_KEY);
        Ok(map)
    }
}

impl TryFrom<&Entity> for AtomicEntity {
    type Error = ProvenanceError;

    fn try_from(entity: &Entity) -> Result<AtomicEntity, ProvenanceError> {
        AtomicEntity::from_entity(entity)
    }
}

impl Serialize for AtomicEntity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_ordered_map().map_err(serde::ser::Error::custom)?.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AtomicEntity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<AtomicEntity, D::Error> {
        let map: IndexMap<String, Value> = IndexMap::deserialize(deserializer)?;
        let id: Uri = map
            .get("id")
            .cloned()
            .map(serde_json::from_value::<Uri>)
            .ok_or_else(|| D::Error::custom("atomic entity missing id"))?
            .map_err(D::Error::custom)?;
        let type_: EntityTypeValue = map
            .get("type")
            .cloned()
            .map(serde_json::from_value::<EntityTypeValue>)
            .ok_or_else(|| D::Error::custom("atomic entity missing type"))?
            .map_err(D::Error::custom)?;
        let proof_value = map
            .get(AtomicEntity::PROOF_KEY)
            .cloned()
            .ok_or_else(|| D::Error::custom("atomic entity missing ngsildproof"))?;
        let proof: NgsildProof = serde_json::from_value(proof_value).map_err(D::Error::custom)?;
        let mut core: Option<(String, Attribute)> = None;
        for (k, v) in map {
            if k == "id" || k == "type" || k == AtomicEntity::PROOF_KEY {
                continue;
            }
            if core.is_some() {
                return Err(D::Error::custom("atomic entity must contain exactly one attribute"));
            }
            let attr: Attribute = serde_json::from_value(v).map_err(D::Error::custom)?;
            core = Some((k, attr));
        }
        let (attribute_name, attribute) = core.ok_or_else(|| D::Error::custom("atomic entity missing core attribute"))?;
        Ok(AtomicEntity {
            head: AtomicEntityHead { id, type_ },
            attribute_name,
            attribute,
            proof,
        })
    }
}

fn attribute_value_as_single(value: &AttributeValue) -> Result<Attribute, ProvenanceError> {
    match value {
        AttributeValue::Single(attr) => Ok((**attr).clone()),
        AttributeValue::Multiple(attrs) => {
            if attrs.len() == 1 {
                Ok(attrs[0].clone())
            } else {
                Err(ProvenanceError::InvalidAttributeCount(attrs.len()))
            }
        }
        AttributeValue::Raw(_) => Err(ProvenanceError::InvalidNgsildProof {
            reason: "raw attribute value cannot be interpreted as atomic core".into(),
        }),
    }
}

fn attribute_value_as_single_json(value: &Value) -> Result<Value, ProvenanceError> {
    match value {
        Value::Array(items) if items.len() == 1 => Ok(items[0].clone()),
        Value::Object(_) => Ok(value.clone()),
        _other => Err(ProvenanceError::InvalidNgsildProof {
            reason: "expected single-instance ngsildproof".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    fn signed_atomic_entity_json() -> serde_json::Value {
        serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": {
                "type": "Property",
                "value": { "streetAddress": "Mühlerstrasse 12" }
            },
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
        })
    }

    fn unsigned_atomic_entity_json() -> serde_json::Value {
        serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": {
                "type": "Property",
                "value": { "streetAddress": "Mühlerstrasse 12" }
            }
        })
    }

    #[test]
    fn atomic_entity_tryfrom_entity_rejects_zero_attributes() {
        let entity: Entity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store"
        }))
        .unwrap();
        assert!(matches!(AtomicEntity::try_from(&entity), Err(ProvenanceError::MissingNgsildProof)));
    }

    #[test]
    fn atomic_entity_tryfrom_entity_rejects_two_attributes() {
        let entity: Entity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1 },
            "name": { "type": "Property", "value": 2 },
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
        let err = AtomicEntity::try_from(&entity).unwrap_err();
        assert!(matches!(err, ProvenanceError::InvalidAttributeCount(_)));
    }

    #[test]
    fn atomic_entity_tryfrom_entity_requires_proof() {
        let entity: Entity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1 }
        }))
        .unwrap();
        assert!(matches!(AtomicEntity::try_from(&entity), Err(ProvenanceError::MissingNgsildProof)));
    }

    #[test]
    fn atomic_entity_serializes_head_core_tail_order() {
        let ae: AtomicEntity = serde_json::from_value(signed_atomic_entity_json()).expect("deserialize");
        let map = ae.to_ordered_map().expect("ordered");
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        pretty_assertions::assert_eq!(keys, vec!["id", "type", "address", "ngsildproof"]);
    }

    #[test]
    fn atomic_entity_roundtrips_signed_example() {
        let original = signed_atomic_entity_json();
        let ae: AtomicEntity = serde_json::from_value(original.clone()).expect("deserialize");
        let roundtrip = serde_json::to_value(&ae).expect("serialize");
        pretty_assertions::assert_eq!(roundtrip["id"], original["id"]);
        pretty_assertions::assert_eq!(roundtrip["type"], original["type"]);
        pretty_assertions::assert_eq!(roundtrip["address"]["type"], "Property");
        pretty_assertions::assert_eq!(roundtrip["ngsildproof"]["type"], "Property");
    }

    #[test]
    fn atomic_entity_unsigned_drops_proof() {
        let ae: AtomicEntity = serde_json::from_value(signed_atomic_entity_json()).unwrap();
        let unsigned = ae.to_unsigned_map().expect("unsigned");
        assert!(!unsigned.contains_key("ngsildproof"));
        let expected_json = unsigned_atomic_entity_json();
        let expected_keys: std::collections::BTreeSet<&str> = expected_json.as_object().unwrap().keys().map(String::as_str).collect();
        let actual_keys: std::collections::BTreeSet<&str> = unsigned.keys().map(String::as_str).collect();
        pretty_assertions::assert_eq!(actual_keys, expected_keys);
    }
}
