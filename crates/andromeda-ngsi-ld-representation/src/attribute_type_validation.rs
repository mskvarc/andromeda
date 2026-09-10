//! Rejecting attribute nodes that are not valid for the type they declare.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4: data that is not syntactically correct
//! according to the API data type definitions is an error of type
//! `BadRequestData`. Two such definitions are decidable on the compact payload
//! a request carries, before the normalization of clause 4.5.2.3 infers a type
//! where none was declared and before JSON-LD expansion rewrites the member
//! names:
//!
//! - Clause 5.2.4 fixes the attribute types to the eight of
//!   [`AttributeType`], so a node that declares any other token --
//!   `"Propertyy"`, `"NotAnAttributeType"` -- is not an attribute and must not
//!   reach the store as one.
//! - Clause 4.5.2.2 admits the `entityIdSealed` and `entityTypeSealed` members
//!   only on the `ngsildproof` Property. That one has to be decided here:
//!   [`crate::proof_seal_admission`] explains why the expanded payload no
//!   longer distinguishes a forged seal from a sub-attribute.
//!
//! What one family's members mean on another -- a Relationship carrying
//! `value`, a Property carrying `object` -- survives expansion intact and is
//! decided there instead, against [`crate::family_member_admission`].

use crate::{
    entity_members::ENTITY_MEMBERS,
    family_member_admission::MemberAdmission,
    normalization::distinguishing_member_type,
    proof_seal_admission::{is_proof_seal, proof_seal_admission},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Map, Value};
use std::str::FromStr;
use thiserror::Error;

/// An attribute node that is not valid for the type it declares.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AttributeTypeError {
    /// The named member declared a `type` token that is not an attribute type.
    #[error("attribute {attribute} declares the type {token}, which is not an NGSI-LD attribute type")]
    NotAnAttributeType {
        /// The entity member carrying the node.
        attribute: String,
        /// The token the node declared.
        token: String,
    },
    /// The named member carried a data-integrity seal outside `ngsildproof`.
    #[error("attribute {attribute} carries the member {member}, which clause 4.5.2.2 admits only on the ngsildproof Property")]
    ProofSealOutsideNgsildproof {
        /// The attribute carrying the seal.
        attribute: String,
        /// The seal member the attribute carried.
        member: String,
    },
}

/// Check every attribute node of an entity or entity fragment.
///
/// Entity members (`id`, `type`, `scope`, ...) are skipped, as is anything that
/// is not an object: a concise value declares no type. A multi-instance array
/// is checked instance by instance. A payload that is not an object is left to
/// whoever validates its shape.
///
/// # Errors
///
/// The first node declaring a token outside clause 5.2.4.
pub fn validate_fragment_attribute_types(fragment: &Value) -> Result<(), AttributeTypeError> {
    let Some(members) = fragment.as_object() else {
        return Ok(());
    };
    members
        .iter()
        .filter(|(member, _)| !ENTITY_MEMBERS.contains(&member.as_str()))
        .try_for_each(|(member, value)| validate_attribute_node(member, value))
}

/// Check one attribute node, as a single-attribute payload carries it
/// (clause 5.6.4: an Attribute Fragment is the node itself).
///
/// # Errors
///
/// The node declares a token outside clause 5.2.4.
pub fn validate_attribute_node(attribute: &str, node: &Value) -> Result<(), AttributeTypeError> {
    match node {
        Value::Object(instance) => validate_instance(attribute, instance),
        Value::Array(instances) => instances
            .iter()
            .filter_map(Value::as_object)
            .try_for_each(|instance| validate_instance(attribute, instance)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Check one attribute instance: the `type` it declares, and the seals it and
/// its sub-attributes carry.
///
/// A `type` that is not a string is the `type` of a concise value object, not a
/// declaration, and is left alone.
fn validate_instance(attribute: &str, instance: &Map<String, Value>) -> Result<(), AttributeTypeError> {
    validate_proof_seals(attribute, instance)?;
    let Some(token) = instance.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    if AttributeType::from_str(token).is_ok() || is_geojson_geometry_type(token) {
        return Ok(());
    }
    Err(AttributeTypeError::NotAnAttributeType {
        attribute: attribute.to_string(),
        token: token.to_string(),
    })
}

/// Reject a data-integrity seal on an attribute that is not `ngsildproof`,
/// here or on any attribute nested under it (clause 4.5.2.2).
///
/// The descent needs the family, because only
/// [`AttributeType::reserved_fields`] says which members belong to the envelope
/// rather than to a sub-attribute: without it a Property's value object would
/// be walked as if it were a nested attribute. An instance that declares no
/// family is checked for its own seals and left at that.
fn validate_proof_seals(attribute: &str, instance: &Map<String, Value>) -> Result<(), AttributeTypeError> {
    // A concise instance declares no `type` (clause 4.5.2.3), so the family is
    // inferred from its distinguishing member the way normalization infers it.
    // Stopping the walk where no `type` is written would leave a seal forged
    // under a concise sub-attribute unexamined, which is the one place it could
    // still reach the store.
    let family = instance
        .get("type")
        .and_then(Value::as_str)
        .and_then(|token| AttributeType::from_str(token).ok())
        .or_else(|| distinguishing_member_type(instance))
        .unwrap_or(AttributeType::Property);
    if let Some(member) = instance
        .keys()
        .find(|member| proof_seal_admission(attribute, family, member) == MemberAdmission::Forbidden)
    {
        return Err(AttributeTypeError::ProofSealOutsideNgsildproof {
            attribute: attribute.to_string(),
            member: member.clone(), // clone: the reported member outlives the borrowed instance
        });
    }
    let reserved = family.reserved_fields();
    instance
        .iter()
        .filter(|(member, _)| member.as_str() != "type" && !reserved.contains(&member.as_str()) && !is_proof_seal(member))
        .try_for_each(|(member, node)| validate_sub_attribute_proof_seals(member, node))
}

/// Walk one sub-attribute slot -- a single node, several instances, or a
/// concise value -- for seals clause 4.5.2.2 does not admit under its name.
fn validate_sub_attribute_proof_seals(attribute: &str, node: &Value) -> Result<(), AttributeTypeError> {
    match node {
        Value::Object(instance) => validate_proof_seals(attribute, instance),
        Value::Array(instances) => instances
            .iter()
            .filter_map(Value::as_object)
            .try_for_each(|instance| validate_proof_seals(attribute, instance)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// The RFC 7946 geometry tokens a bare `GeoJSON` geometry value declares as its
/// `type` (clause 4.5.2.3 geospatial exception): such an object is the value
/// of a `GeoProperty`, not an attribute node, and is not a declaration to check.
fn is_geojson_geometry_type(token: &str) -> bool {
    matches!(
        token,
        "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" | "GeometryCollection"
    )
}

#[cfg(test)]
mod tests {
    use crate::attribute_type_validation::{AttributeTypeError, validate_attribute_node, validate_fragment_attribute_types};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::json;

    #[test]
    fn every_clause_4_5_attribute_type_is_accepted() {
        for attribute_type in AttributeType::ALL {
            let fragment = json!({ "speed": { "type": attribute_type.to_string(), "value": 1 } });
            assert!(validate_fragment_attribute_types(&fragment).is_ok(), "{attribute_type}");
        }
    }

    #[test]
    fn a_token_outside_clause_5_2_4_is_rejected_with_its_member_and_token() {
        let fragment = json!({ "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "odd": { "type": "NotAnAttributeType", "value": 1 } });
        pretty_assertions::assert_eq!(
            validate_fragment_attribute_types(&fragment),
            Err(AttributeTypeError::NotAnAttributeType {
                attribute: "odd".to_string(),
                token: "NotAnAttributeType".to_string(),
            })
        );
    }

    #[test]
    fn entity_members_concise_values_and_geometries_are_not_declarations() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/a",
            "speed": 42,
            "location": { "type": "Point", "coordinates": [1.0, 2.0] },
            "raw": { "type": 7, "value": 1 }
        });
        assert!(validate_fragment_attribute_types(&fragment).is_ok());
        assert!(validate_fragment_attribute_types(&json!("not an object")).is_ok());
    }

    #[test]
    fn a_multi_instance_array_is_checked_instance_by_instance() {
        let instances = json!([
            { "type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a" },
            { "type": "Bogus", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:b" }
        ]);
        pretty_assertions::assert_eq!(
            validate_attribute_node("speed", &instances),
            Err(AttributeTypeError::NotAnAttributeType {
                attribute: "speed".to_string(),
                token: "Bogus".to_string(),
            })
        );
    }

    /// Clause 4.5.2.2: `entityIdSealed` and `entityTypeSealed` "shall never be
    /// present, unless the Property name is `ngsildproof`".
    #[test]
    fn a_data_integrity_seal_on_an_attribute_that_is_not_ngsildproof_is_rejected() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let fragment = json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 1, member: "urn:ngsi-ld:Vehicle:1" }
            });
            pretty_assertions::assert_eq!(
                validate_fragment_attribute_types(&fragment),
                Err(AttributeTypeError::ProofSealOutsideNgsildproof {
                    attribute: "speed".to_string(),
                    member: member.to_string(),
                }),
                "{member}"
            );
        }
    }

    /// The same two members under the `ngsildproof` Property clause 4.5.2.2
    /// defines them for are the intended shape and are accepted.
    #[test]
    fn the_seals_of_the_ngsildproof_property_are_accepted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 1,
                "ngsildproof": {
                    "type": "Property",
                    "value": { "type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022" },
                    "entityIdSealed": "urn:ngsi-ld:Vehicle:1",
                    "entityTypeSealed": "Vehicle"
                }
            }
        });
        assert!(validate_fragment_attribute_types(&fragment).is_ok());
    }

    /// A seal is refused however deep it is buried, because a sub-attribute is
    /// itself an attribute (clause 4.5.2.2) and carries the same prohibition.
    #[test]
    fn a_seal_on_a_sub_attribute_is_rejected_under_the_sub_attributes_name() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 1, "accuracy": { "type": "Property", "value": 0.5, "entityIdSealed": "urn:ngsi-ld:Vehicle:1" } }
        });
        pretty_assertions::assert_eq!(
            validate_fragment_attribute_types(&fragment),
            Err(AttributeTypeError::ProofSealOutsideNgsildproof {
                attribute: "accuracy".to_string(),
                member: "entityIdSealed".to_string(),
            })
        );
    }

    /// A Property's value is a value, not a nested attribute, so a member
    /// named like a seal inside it is data rather than a forged seal.
    #[test]
    fn a_seal_name_inside_a_property_value_is_not_a_seal() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": { "type": "Property", "value": { "entityIdSealed": "urn:ngsi-ld:Vehicle:1" } }
        });
        assert!(validate_fragment_attribute_types(&fragment).is_ok());
    }

    #[test]
    fn a_single_attribute_fragment_is_checked_as_the_node_itself() {
        assert!(validate_attribute_node("speed", &json!({ "value": 99 })).is_ok());
        assert!(validate_attribute_node("speed", &json!({ "type": "Relationship", "object": "urn:ngsi-ld:X:1" })).is_ok());
        assert!(validate_attribute_node("speed", &json!({ "type": "Propertyy", "value": 1 })).is_err());
    }
}
