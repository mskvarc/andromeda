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
    proof_seal_admission::{ProofSealMember, is_proof_seal, proof_seal_admission},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, PathSegment};
use serde_json::{Map, Value};
use std::str::FromStr;
use thiserror::Error;

/// An attribute node that is not valid for the type it declares.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AttributeTypeError {
    /// The attribute node declared a `type` token that is not an attribute type.
    #[error("the attribute at {attribute} declares a type that is not an NGSI-LD attribute type")]
    NotAnAttributeType {
        /// Where the attribute node stands in the payload.
        attribute: JsonPointer,
        /// Why the declared token is not an attribute type.
        #[source]
        source: strum::ParseError,
    },
    /// The attribute node carried a data-integrity seal outside `ngsildproof` (ETSI GS CIM 009 clause 4.5.2.2).
    #[error("the attribute at {attribute} carries the member {member}, which is admitted only on the ngsildproof Property")]
    ProofSealOutsideNgsildproof {
        /// Where the attribute node carrying the seal stands in the payload.
        attribute: JsonPointer,
        /// The seal member the attribute carried.
        member: ProofSealMember,
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
    let mut path = vec![PathSegment::Member(attribute)];
    validate_attribute_slot(attribute, node, &mut path, validate_instance)
}

/// The check one attribute instance undergoes, given the attribute's name and
/// the location of the instance.
type InstanceCheck = for<'a> fn(&'a str, &'a Map<String, Value>, &mut Vec<PathSegment<'a>>) -> Result<(), AttributeTypeError>;

/// Run `check` over one attribute slot: a single node, or every instance of a
/// multi-instance array. A concise value is no node and is left alone.
fn validate_attribute_slot<'a>(attribute: &'a str, node: &'a Value, path: &mut Vec<PathSegment<'a>>, check: InstanceCheck) -> Result<(), AttributeTypeError> {
    match node {
        Value::Object(instance) => check(attribute, instance, path),
        Value::Array(instances) => instances.iter().enumerate().try_for_each(|(index, instance)| {
            let Some(instance) = instance.as_object() else {
                return Ok(());
            };
            path.push(PathSegment::Index(index));
            let outcome = check(attribute, instance, path);
            path.pop();
            outcome
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Check one attribute instance: the `type` it declares, and the seals it and
/// its sub-attributes carry.
///
/// A `type` that is not a string is the `type` of a concise value object, not a
/// declaration, and is left alone.
fn validate_instance<'a>(attribute: &'a str, instance: &'a Map<String, Value>, path: &mut Vec<PathSegment<'a>>) -> Result<(), AttributeTypeError> {
    validate_proof_seals(attribute, instance, path)?;
    let Some(token) = instance.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    if is_geojson_geometry_type(token) {
        return Ok(());
    }
    AttributeType::from_str(token)
        .map(|_| ())
        .map_err(|source| AttributeTypeError::NotAnAttributeType {
            attribute: JsonPointer::from_walk(path),
            source,
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
fn validate_proof_seals<'a>(attribute: &'a str, instance: &'a Map<String, Value>, path: &mut Vec<PathSegment<'a>>) -> Result<(), AttributeTypeError> {
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
        .filter(|member| proof_seal_admission(attribute, family, member) == MemberAdmission::Forbidden)
        .find_map(|member| ProofSealMember::of(member))
    {
        return Err(AttributeTypeError::ProofSealOutsideNgsildproof {
            attribute: JsonPointer::from_walk(path),
            member,
        });
    }
    let reserved = family.reserved_fields();
    instance
        .iter()
        .filter(|(member, _)| member.as_str() != "type" && !reserved.contains(&member.as_str()) && !is_proof_seal(member))
        .try_for_each(|(member, node)| {
            path.push(PathSegment::Member(member));
            let outcome = validate_attribute_slot(member, node, path, validate_proof_seals);
            path.pop();
            outcome
        })
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
    use crate::{
        attribute_type_validation::{AttributeTypeError, validate_attribute_node, validate_fragment_attribute_types},
        proof_seal_admission::ProofSealMember,
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, PathSegment};
    use serde_json::json;

    /// The pointer a walk reports for the given member names and array indexes.
    fn at(segments: &[PathSegment<'_>]) -> JsonPointer {
        JsonPointer::from_walk(segments)
    }

    /// The seal a test fixture wrote, as the error reports it.
    fn seal(member: &str) -> ProofSealMember {
        ProofSealMember::of(member).unwrap_or_else(|| panic!("{member} is a seal"))
    }

    #[test]
    fn every_clause_4_5_attribute_type_is_accepted() {
        for attribute_type in AttributeType::ALL {
            let fragment = json!({ "speed": { "type": attribute_type.to_string(), "value": 1 } });
            assert!(validate_fragment_attribute_types(&fragment).is_ok(), "{attribute_type}");
        }
    }

    #[test]
    fn a_token_outside_clause_5_2_4_is_rejected_at_the_attribute_declaring_it() {
        let fragment = json!({ "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "odd": { "type": "NotAnAttributeType", "value": 1 } });
        pretty_assertions::assert_eq!(
            validate_fragment_attribute_types(&fragment),
            Err(AttributeTypeError::NotAnAttributeType {
                attribute: at(&[PathSegment::Member("odd")]),
                source: strum::ParseError::VariantNotFound,
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
                attribute: at(&[PathSegment::Member("speed"), PathSegment::Index(1)]),
                source: strum::ParseError::VariantNotFound,
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
                    attribute: at(&[PathSegment::Member("speed")]),
                    member: seal(member),
                }),
                "{member}"
            );
        }
    }

    /// Clause 4.5.3.2, "Prohibited": on a Relationship in the normalized
    /// representation, "`entityIdSealed` and `entityTypeSealed` shall never be
    /// present". The clause states it without the `ngsildproof` exception
    /// clause 4.5.2.2 grants a Property, because that exception is a
    /// Property's.
    #[test]
    fn a_data_integrity_seal_on_a_normalized_relationship_is_rejected() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let fragment = json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "belongsTo": { "type": "Relationship", "object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Vehicle:1" }
            });
            pretty_assertions::assert_eq!(
                validate_fragment_attribute_types(&fragment),
                Err(AttributeTypeError::ProofSealOutsideNgsildproof {
                    attribute: at(&[PathSegment::Member("belongsTo")]),
                    member: seal(member),
                }),
                "{member}"
            );
        }
    }

    /// Clause 4.5.3.3 repeats the same prohibition for the concise
    /// representation, where a Relationship declares no `type` at all. The
    /// family the rule needs comes from
    /// [`distinguishing_member_type`](crate::normalization::distinguishing_member_type),
    /// which reads the `object` member the clause makes the Relationship's
    /// distinguishing one -- so the seal rule is reached on a node that names
    /// no family of its own.
    #[test]
    fn a_data_integrity_seal_on_a_concise_relationship_is_rejected_through_family_inference() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let fragment = json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "belongsTo": { "object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Vehicle:1" }
            });
            pretty_assertions::assert_eq!(
                validate_fragment_attribute_types(&fragment),
                Err(AttributeTypeError::ProofSealOutsideNgsildproof {
                    attribute: at(&[PathSegment::Member("belongsTo")]),
                    member: seal(member),
                }),
                "{member}"
            );
        }
    }

    /// The name alone does not admit a seal: clauses 4.5.3.2 and 4.5.3.3
    /// prohibit both members on a Relationship with no exception, and clause
    /// 4.5.2.2 defines `ngsildproof` as a Property. An attribute that takes the
    /// name and declares -- or, concisely, implies -- a Relationship carries no
    /// admissible seal.
    #[test]
    fn a_seal_on_an_ngsildproof_that_is_a_relationship_is_rejected_in_both_representations() {
        for node in [
            json!({ "type": "Relationship", "object": "urn:ngsi-ld:Proof:1", "entityIdSealed": "urn:ngsi-ld:Vehicle:1" }),
            json!({ "object": "urn:ngsi-ld:Proof:1", "entityIdSealed": "urn:ngsi-ld:Vehicle:1" }),
        ] {
            let fragment = json!({ "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "ngsildproof": node });
            pretty_assertions::assert_eq!(
                validate_fragment_attribute_types(&fragment),
                Err(AttributeTypeError::ProofSealOutsideNgsildproof {
                    attribute: at(&[PathSegment::Member("ngsildproof")]),
                    member: ProofSealMember::EntityIdSealed,
                })
            );
        }
    }

    /// A seal forged on a concise Relationship written as a sub-attribute is
    /// refused too: clause 4.5.2.2 makes a sub-attribute an attribute, and the
    /// walk infers its family the same way.
    #[test]
    fn a_seal_on_a_concise_relationship_sub_attribute_is_rejected() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 1, "providedBy": { "object": "urn:ngsi-ld:Device:1", "entityTypeSealed": "Vehicle" } }
        });
        pretty_assertions::assert_eq!(
            validate_fragment_attribute_types(&fragment),
            Err(AttributeTypeError::ProofSealOutsideNgsildproof {
                attribute: at(&[PathSegment::Member("speed"), PathSegment::Member("providedBy")]),
                member: ProofSealMember::EntityTypeSealed,
            })
        );
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
                attribute: at(&[PathSegment::Member("speed"), PathSegment::Member("accuracy")]),
                member: ProofSealMember::EntityIdSealed,
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
