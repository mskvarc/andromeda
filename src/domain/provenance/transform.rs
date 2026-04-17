use crate::domain::{
    attribute::{
        Attribute,
        AttributeValue,
        Property,
        common::{AttributeSystemFields, CommonAttributeFields},
    },
    entity::Entity,
    provenance::{
        atomic_entity::{AtomicEntity, AtomicEntityHead},
        error::ProvenanceError,
        ngsild_proof::NgsildProof,
    },
};

/// Derive a sealed attribute from a core attribute and its proof
/// (ETSI GS CIM 019 v1.2.1 clause 5.2.3). The proof is attached to the
/// attribute's `common.ngsildproof` sub-property.
///
/// Only used by test fixtures: INT-4 forbids broker-side signing, so
/// production code never derives sealed attributes. The inverse
/// [`reconstruct_atomic_entity`] is the production verification path.
#[cfg(test)]
pub fn derive_sealed_attribute(mut attribute: Attribute, proof: NgsildProof) -> Result<Attribute, ProvenanceError> {
    let proof_json = serde_json::to_value(&proof).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    let property: Property = serde_json::from_value(proof_json).map_err(|e| ProvenanceError::InvalidNgsildProof { reason: e.to_string() })?;
    common_mut(&mut attribute).ngsildproof = Some(Box::new(property));
    Ok(attribute)
}

/// Parse an [`NgsildProof`] out of a [`Property`] that was stored as the
/// `common.ngsildproof` sub-property. Enforces the `type` literals from
/// CIM 019 clause 5.2.1 (outer = `"Property"`) and clause 5.1.1 (inner
/// proof `type` = `"DataIntegrityProof"`) with dedicated error variants
/// ahead of structural serde deserialisation.
pub fn parse_ngsild_proof_from_property(property: &Property) -> Result<NgsildProof, ProvenanceError> {
    let json = serde_json::to_value(property).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    let outer_type = json.get("type").and_then(|v| v.as_str());
    if outer_type != Some("Property") {
        return Err(ProvenanceError::InvalidNgsildProofType);
    }
    let inner_type = json.get("value").and_then(|v| v.get("type")).and_then(|v| v.as_str());
    if inner_type != Some("DataIntegrityProof") {
        return Err(ProvenanceError::InvalidProofType);
    }
    serde_json::from_value(json).map_err(|e| ProvenanceError::InvalidNgsildProof { reason: e.to_string() })
}

/// Reconstruct an Atomic Entity from a sealed attribute
/// (ETSI GS CIM 019 v1.2.1 clause 5.2.4). The ngsildproof sub-property
/// is stripped from the attribute and moved to the tail of the atomic
/// entity; if the proof carried `entityIdSealed` or `entityTypeSealed`
/// those values override the supplied head.
pub fn reconstruct_atomic_entity(head: AtomicEntityHead, attribute_name: String, mut attribute: Attribute) -> Result<AtomicEntity, ProvenanceError> {
    let common = common_mut(&mut attribute);
    let proof_property = common.ngsildproof.take().ok_or(ProvenanceError::MissingNgsildProof)?;
    // Broker-managed system fields (createdAt, modifiedAt, instanceId,
    // deletedAt) are not part of the producer-signed atomic entity and
    // must be excluded from the canonicalised input. The producer signs
    // before the broker stamps these fields, so the verifier strips
    // them before recomputing the digest.
    common.system = AttributeSystemFields::default();
    let proof = parse_ngsild_proof_from_property(&proof_property)?;

    let AtomicEntityHead { mut id, mut type_ } = head;
    if let Some(sealed) = proof.entity_id_sealed.as_ref() {
        id = sealed.clone();
    }
    if let Some(sealed) = proof.entity_type_sealed.as_ref() {
        type_ = sealed.clone();
    }

    Ok(AtomicEntity {
        head: AtomicEntityHead { id, type_ },
        attribute_name,
        attribute,
        proof,
    })
}

/// Reconstruct an atomic entity directly from an [`Entity`] and a named
/// sealed attribute key.
pub fn reconstruct_atomic_entity_from_entity(entity: &Entity, attribute_name: &str) -> Result<AtomicEntity, ProvenanceError> {
    let value = entity.attrs.get(attribute_name).ok_or_else(|| ProvenanceError::InvalidNgsildProof {
        reason: format!("attribute {attribute_name} not found"),
    })?;
    let attribute = match value {
        AttributeValue::Single(attr) => (**attr).clone(),
        AttributeValue::Multiple(attrs) if attrs.len() == 1 => attrs[0].clone(),
        AttributeValue::Multiple(attrs) => return Err(ProvenanceError::InvalidAttributeCount(attrs.len())),
        AttributeValue::Raw(_) => {
            return Err(ProvenanceError::InvalidNgsildProof {
                reason: "cannot seal raw attribute value".into(),
            });
        }
    };
    let head = AtomicEntityHead {
        id: entity.id.clone(),
        type_: entity.type_.clone(),
    };
    reconstruct_atomic_entity(head, attribute_name.to_string(), attribute)
}

fn common_mut(attribute: &mut Attribute) -> &mut CommonAttributeFields {
    match attribute {
        Attribute::Property(a) => &mut a.common,
        Attribute::Relationship(a) => &mut a.common,
        Attribute::VocabProperty(a) => &mut a.common,
        Attribute::GeoProperty(a) => &mut a.common,
        Attribute::LanguageProperty(a) => &mut a.common,
        Attribute::JsonProperty(a) => &mut a.common,
        Attribute::ListProperty(a) => &mut a.common,
        Attribute::ListRelationship(a) => &mut a.common,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::domain::{entity::EntityTypeValue, uri::Uri};
    use indexmap::IndexMap;
    use std::str::FromStr;

    fn sample_proof() -> NgsildProof {
        serde_json::from_value(serde_json::json!({
            "type": "Property",
            "value": {
                "type": "DataIntegrityProof",
                "cryptosuite": "eddsa-jcs-2022",
                "verificationMethod": "https://example.com/keys/1",
                "proofPurpose": "assertionMethod",
                "proofValue": "z3MvGcVxzRzzpKF1HA11EjvfPZsN8NAb7kXBRfeTm3CFg2Ek4DTPeBoU65yTPewGhP7FP1p"
            }
        }))
        .expect("proof")
    }

    fn sample_property_attribute() -> Attribute {
        serde_json::from_value(serde_json::json!({
            "type": "Property",
            "value": 42
        }))
        .expect("attribute")
    }

    fn sample_relationship_attribute() -> Attribute {
        serde_json::from_value(serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1"
        }))
        .expect("relationship")
    }

    #[test]
    fn derive_sealed_attribute_attaches_ngsildproof_sub_property() {
        let sealed = derive_sealed_attribute(sample_property_attribute(), sample_proof()).expect("derive");
        let Attribute::Property(ref p) = sealed else {
            unreachable!("expected Property");
        };
        assert!(p.common.ngsildproof.is_some());
    }

    #[test]
    fn derive_sealed_attribute_handles_relationship_variant() {
        let sealed = derive_sealed_attribute(sample_relationship_attribute(), sample_proof()).expect("derive");
        let Attribute::Relationship(ref r) = sealed else {
            unreachable!("expected Relationship");
        };
        assert!(r.common.ngsildproof.is_some());
    }

    #[test]
    fn reconstruct_atomic_entity_strips_broker_managed_system_fields() {
        let mut sealed = derive_sealed_attribute(sample_property_attribute(), sample_proof()).unwrap();
        if let Attribute::Property(ref mut p) = sealed {
            p.common.system.created_at = Some(chrono::Utc::now());
            p.common.system.modified_at = Some(chrono::Utc::now());
            p.common.system.instance_id = Some(Uri::from_str("urn:ngsi-ld:attribute:instance:1").unwrap());
        }
        let head = AtomicEntityHead {
            id: Uri::from_str("urn:ngsi-ld:Store:002").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
        };
        let ae = reconstruct_atomic_entity(head, "t".to_string(), sealed).expect("reconstruct");
        let Attribute::Property(ref p) = ae.attribute else {
            unreachable!("expected Property");
        };
        assert!(p.common.system.created_at.is_none(), "createdAt must be stripped from signed payload");
        assert!(p.common.system.modified_at.is_none(), "modifiedAt must be stripped");
        assert!(p.common.system.instance_id.is_none(), "instanceId must be stripped");
    }

    #[test]
    fn reconstruct_atomic_entity_inverts_derive_for_property() {
        let sealed = derive_sealed_attribute(sample_property_attribute(), sample_proof()).unwrap();
        let head = AtomicEntityHead {
            id: Uri::from_str("urn:ngsi-ld:Store:002").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
        };
        let ae = reconstruct_atomic_entity(head, "temperature".to_string(), sealed).expect("reconstruct");
        pretty_assertions::assert_eq!(ae.attribute_name, "temperature");
    }

    #[test]
    fn reconstruct_atomic_entity_inverts_derive_for_relationship() {
        let sealed = derive_sealed_attribute(sample_relationship_attribute(), sample_proof()).unwrap();
        let head = AtomicEntityHead {
            id: Uri::from_str("urn:ngsi-ld:Store:002").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
        };
        let ae = reconstruct_atomic_entity(head, "parent".to_string(), sealed).expect("reconstruct");
        assert!(matches!(ae.attribute, Attribute::Relationship(_)));
    }

    #[test]
    fn reconstruct_atomic_entity_rejects_attribute_without_ngsildproof() {
        let head = AtomicEntityHead {
            id: Uri::from_str("urn:ngsi-ld:Store:002").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
        };
        let err = reconstruct_atomic_entity(head, "t".to_string(), sample_property_attribute()).unwrap_err();
        assert!(matches!(err, ProvenanceError::MissingNgsildProof));
    }

    #[test]
    fn reconstruct_applies_entity_id_sealed_from_proof() {
        let mut proof = sample_proof();
        proof.entity_id_sealed = Some(Uri::from_str("urn:ngsi-ld:Store:sealed").unwrap());
        let sealed = derive_sealed_attribute(sample_property_attribute(), proof).unwrap();
        let head = AtomicEntityHead {
            id: Uri::from_str("urn:ngsi-ld:Store:other").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
        };
        let ae = reconstruct_atomic_entity(head, "t".to_string(), sealed).expect("reconstruct");
        pretty_assertions::assert_eq!(ae.head.id.to_string(), "urn:ngsi-ld:Store:sealed");
    }

    #[test]
    fn reconstruct_atomic_entity_rejects_ngsildproof_with_wrong_type() {
        let mut attr = sample_property_attribute();
        let bogus_property: Property = serde_json::from_value(serde_json::json!({
            "type": "Property",
            "value": { "not": "a proof" }
        }))
        .unwrap();
        common_mut(&mut attr).ngsildproof = Some(Box::new(bogus_property));
        let head = AtomicEntityHead {
            id: Uri::from_str("urn:ngsi-ld:Store:002").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
        };
        assert!(reconstruct_atomic_entity(head, "t".to_string(), attr).is_err());
    }

    #[test]
    fn reconstruct_from_entity_pulls_attribute_by_name() {
        let sealed = derive_sealed_attribute(sample_property_attribute(), sample_proof()).unwrap();
        let mut attrs: IndexMap<String, AttributeValue> = IndexMap::new();
        attrs.insert("temperature".to_string(), AttributeValue::Single(Box::new(sealed)));
        let entity = Entity {
            id: Uri::from_str("urn:ngsi-ld:Store:002").unwrap(),
            type_: EntityTypeValue::Single("Store".into()),
            scope: None,
            expires_at: None,
            location: None,
            observation_space: None,
            operation_space: None,
            created_at: None,
            modified_at: None,
            deleted_at: None,
            attrs,
        };
        let ae = reconstruct_atomic_entity_from_entity(&entity, "temperature").expect("reconstruct");
        pretty_assertions::assert_eq!(ae.attribute_name, "temperature");
    }
}
