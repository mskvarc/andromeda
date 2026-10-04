//! Rejecting compact attribute nodes the specification does not admit.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4: data that is not syntactically correct
//! according to the API data type definitions is an error of type
//! `BadRequestData`. Two such definitions are decidable on the compact payload
//! a request carries, before the normalization of clause 4.5.2.3 infers a type
//! where none was declared and before JSON-LD expansion rewrites the member
//! names:
//!
//! - Every node standing where an attribute or a sub-attribute stands must be
//!   one [`crate::concise_reading`] reads: clause 5.2.4 fixes the attribute
//!   types to the eight of [`AttributeType`], clause 4.5.2.3 treats an object
//!   carrying a `type` as a normalized attribute, and clause 4.5.5 writes the
//!   instances of a multi-attribute as an array of instances. So a node that
//!   declares any other token -- `"Propertyy"`, `"Feature"`, `7` -- names a
//!   geometry without its coordinates, carries the value members of two
//!   families, or mixes instances with values in one array is not an attribute
//!   and must not reach the store as one. A sub-attribute is an attribute
//!   (clause 4.5.2.2), so the rule holds at every depth; a value is data and is
//!   never walked into.
//! - Clause 4.5.2.2 admits the `entityIdSealed` and `entityTypeSealed` members
//!   only on the `ngsildproof` Property. That one has to be decided here:
//!   [`crate::proof_seal_admission`] explains why the expanded payload no
//!   longer distinguishes a forged seal from a sub-attribute.
//!
//! What one family's members mean on another -- a Relationship carrying
//! `value`, a Property carrying `object` -- survives expansion intact and is
//! decided there instead, against [`crate::family_member_admission`].

use crate::{
    concise_reading::{ArrayRefusal, ObjectReading, ObjectRefusal, read_array, read_object},
    entity_members::ENTITY_MEMBERS,
    family_member_admission::MemberAdmission,
    proof_seal_admission::{ProofSealMember, is_proof_seal, proof_seal_admission},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, PathSegment};
use serde_json::{Map, Value};
use thiserror::Error;

/// A compact attribute node the specification does not admit, and where it
/// stands.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("the attribute at {attribute} {fault}")]
pub struct AttributeTypeError {
    /// Where the attribute node stands in the payload.
    pub attribute: JsonPointer,
    /// Why the node is not admitted.
    pub fault: AttributeNodeFault,
}

/// Why a compact attribute node is not admitted.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AttributeNodeFault {
    /// The node declared a `type` token that is neither an attribute type
    /// (clause 5.2.4) nor a geometry type.
    #[error("declares a type that is not an NGSI-LD attribute type")]
    NotAnAttributeType(#[source] strum::ParseError),
    /// The node declared a `type` that is not a string.
    #[error("declares a type that is not a string")]
    TypeNotAString,
    /// The node named a `GeoJSON` geometry type without the member holding the
    /// geometry's coordinates (clause 4.7.1).
    #[error("names a GeoJSON geometry type without the member holding its coordinates")]
    GeometryWithoutCoordinates,
    /// The node declared no `type` and carried the value members of several
    /// attribute types.
    #[error("declares no type and carries the value members of several attribute types")]
    SeveralValueMembers,
    /// The node is an array holding both attribute instances and values
    /// (clause 4.5.5).
    #[error("mixes attribute instances with values in one array")]
    MixedInstancesAndValues,
    /// The node carried a data-integrity seal outside `ngsildproof` (clause
    /// 4.5.2.2).
    #[error("carries the member {0}, which is admitted only on the ngsildproof Property")]
    ProofSealOutsideNgsildproof(ProofSealMember),
}

/// Check every attribute node of an entity or entity fragment.
///
/// Entity members (`id`, `type`, `scope`, ...) are skipped. A payload that is
/// not an object is left to whoever validates its shape.
///
/// # Errors
///
/// The first node the specification does not admit.
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
/// (clause 5.6.4: an Attribute Fragment is the node itself), together with
/// every attribute nested in it.
///
/// # Errors
///
/// The first node the specification does not admit.
pub fn validate_attribute_node(attribute: &str, node: &Value) -> Result<(), AttributeTypeError> {
    let mut path = vec![PathSegment::Member(attribute)];
    validate_slot(attribute, node, &mut path)
}

/// Check one attribute slot: a single node, the instances of a multi-attribute,
/// or a value.
fn validate_slot<'a>(attribute: &'a str, node: &'a Value, path: &mut Vec<PathSegment<'a>>) -> Result<(), AttributeTypeError> {
    match node {
        Value::Object(object) => validate_object(attribute, object, path),
        Value::Array(items) => {
            read_array(items).map_err(|refusal| refused_array(refusal, path))?;
            items.iter().enumerate().try_for_each(|(index, item)| {
                let Value::Object(object) = item else {
                    return Ok(());
                };
                path.push(PathSegment::Index(index));
                let outcome = validate_object(attribute, object, path);
                path.pop();
                outcome
            })
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Check one object standing in an attribute slot, and, when it is an
/// instance, everything nested in it.
fn validate_object<'a>(attribute: &'a str, object: &'a Map<String, Value>, path: &mut Vec<PathSegment<'a>>) -> Result<(), AttributeTypeError> {
    match read_object(object).map_err(|refusal| refused(path, object_fault(refusal)))? {
        ObjectReading::Instance(family) => validate_instance(attribute, family, object, path),
        ObjectReading::GeoPropertyValue | ObjectReading::PropertyValue => Ok(()),
    }
}

/// Check one instance of `family`: the seals it carries, and every
/// sub-attribute.
///
/// Only [`AttributeType::reserved_fields`] says which members belong to the
/// envelope rather than to a sub-attribute, which is why the family is needed:
/// without it a Property's value object would be walked as if it were a nested
/// attribute.
fn validate_instance<'a>(
    attribute: &'a str,
    family: AttributeType,
    instance: &'a Map<String, Value>,
    path: &mut Vec<PathSegment<'a>>,
) -> Result<(), AttributeTypeError> {
    if let Some(member) = instance
        .keys()
        .filter(|member| proof_seal_admission(attribute, family, member) == MemberAdmission::Forbidden)
        .find_map(|member| ProofSealMember::of(member))
    {
        return Err(refused(path, AttributeNodeFault::ProofSealOutsideNgsildproof(member)));
    }
    let reserved = family.reserved_fields();
    instance
        .iter()
        .filter(|(member, _)| member.as_str() != "type" && !reserved.contains(&member.as_str()) && !is_proof_seal(member))
        .try_for_each(|(member, node)| {
            path.push(PathSegment::Member(member));
            let outcome = validate_slot(member, node, path);
            path.pop();
            outcome
        })
}

/// The refusal of the node standing at `path`.
fn refused(path: &[PathSegment<'_>], fault: AttributeNodeFault) -> AttributeTypeError {
    AttributeTypeError {
        attribute: JsonPointer::from_walk(path),
        fault,
    }
}

/// The refusal of the array standing at `path`, located at the element it
/// names when it names one.
fn refused_array(refusal: ArrayRefusal, path: &mut Vec<PathSegment<'_>>) -> AttributeTypeError {
    match refusal {
        ArrayRefusal::Element { index, refusal } => {
            path.push(PathSegment::Index(index));
            let error = refused(path, object_fault(refusal));
            path.pop();
            error
        }
        ArrayRefusal::MixedInstancesAndValues => refused(path, AttributeNodeFault::MixedInstancesAndValues),
    }
}

/// The fault an object the reading refuses is reported with.
const fn object_fault(refusal: ObjectRefusal) -> AttributeNodeFault {
    match refusal {
        ObjectRefusal::NotAnAttributeType(source) => AttributeNodeFault::NotAnAttributeType(source),
        ObjectRefusal::TypeNotAString => AttributeNodeFault::TypeNotAString,
        ObjectRefusal::GeometryWithoutCoordinates => AttributeNodeFault::GeometryWithoutCoordinates,
        ObjectRefusal::SeveralValueMembers => AttributeNodeFault::SeveralValueMembers,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_type_validation::{AttributeNodeFault, AttributeTypeError, validate_attribute_node, validate_fragment_attribute_types},
        proof_seal_admission::ProofSealMember,
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, PathSegment};
    use serde_json::json;

    /// The refusal a walk reports for the given member names and array
    /// indexes.
    fn refusal(segments: &[PathSegment<'_>], fault: AttributeNodeFault) -> AttributeTypeError {
        AttributeTypeError {
            attribute: JsonPointer::from_walk(segments),
            fault,
        }
    }

    /// The fault of a `type` token outside clause 5.2.4.
    const fn not_an_attribute_type() -> AttributeNodeFault {
        AttributeNodeFault::NotAnAttributeType(strum::ParseError::VariantNotFound)
    }

    /// The seal a test fixture wrote, as the error reports it.
    fn seal(member: &str) -> AttributeNodeFault {
        AttributeNodeFault::ProofSealOutsideNgsildproof(ProofSealMember::of(member).unwrap_or_else(|| panic!("{member} is a seal")))
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
            Err(refusal(&[PathSegment::Member("odd")], not_an_attribute_type()))
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
            "raw": { "other": 7, "value": 1 }
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
            Err(refusal(&[PathSegment::Member("speed"), PathSegment::Index(1)], not_an_attribute_type()))
        );
    }

    /// Clause 4.5.2.2 makes a sub-attribute "the result of serializing a
    /// Property", so its `type` is held to clause 5.2.4 where it stands.
    #[test]
    fn a_misspelt_sub_attribute_type_is_rejected_where_it_stands() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 1, "accuracy": { "type": "Propertyy", "value": 0.5 } }
        });
        pretty_assertions::assert_eq!(
            validate_fragment_attribute_types(&fragment),
            Err(refusal(
                &[PathSegment::Member("speed"), PathSegment::Member("accuracy")],
                not_an_attribute_type()
            ))
        );
    }

    /// A concise instance -- one whose family `value` implies -- has its
    /// sub-attributes checked as a declared one does.
    #[test]
    fn a_sub_attribute_of_a_concise_instance_is_checked_too() {
        let node = json!({ "value": 1, "accuracy": { "type": "Feature", "x": 1 } });
        pretty_assertions::assert_eq!(
            validate_attribute_node("speed", &node),
            Err(refusal(
                &[PathSegment::Member("speed"), PathSegment::Member("accuracy")],
                not_an_attribute_type()
            ))
        );
    }

    /// Clause 4.5.2.3 reads an object carrying a `type` as a normalized
    /// attribute, whose `type` Table 5.2.5-1 makes a String.
    #[test]
    fn a_type_that_is_not_a_string_is_rejected() {
        pretty_assertions::assert_eq!(
            validate_attribute_node("raw", &json!({ "type": 7, "value": 1 })),
            Err(refusal(&[PathSegment::Member("raw")], AttributeNodeFault::TypeNotAString))
        );
        pretty_assertions::assert_eq!(
            validate_attribute_node(
                "speed",
                &json!({ "type": "Property", "value": 1, "accuracy": { "type": ["Property"], "value": 0.5 } })
            ),
            Err(refusal(
                &[PathSegment::Member("speed"), PathSegment::Member("accuracy")],
                AttributeNodeFault::TypeNotAString
            ))
        );
    }

    #[test]
    fn a_geometry_type_without_coordinates_is_rejected() {
        pretty_assertions::assert_eq!(
            validate_attribute_node("location", &json!({ "type": "Point" })),
            Err(refusal(&[PathSegment::Member("location")], AttributeNodeFault::GeometryWithoutCoordinates))
        );
    }

    #[test]
    fn two_value_members_on_a_typeless_attribute_are_rejected() {
        pretty_assertions::assert_eq!(
            validate_attribute_node("ambiguous", &json!({ "object": "urn:ngsi-ld:X:1", "languageMap": { "en": "x" } })),
            Err(refusal(&[PathSegment::Member("ambiguous")], AttributeNodeFault::SeveralValueMembers))
        );
    }

    /// Clause 4.5.5 writes several instances as an array of instances; an array
    /// holding instances and values as well is neither that nor one value.
    #[test]
    fn an_array_mixing_instances_and_values_is_rejected() {
        pretty_assertions::assert_eq!(
            validate_attribute_node("speed", &json!([{ "value": 25, "datasetId": "urn:ngsi-ld:Dataset:a" }, 30])),
            Err(refusal(&[PathSegment::Member("speed")], AttributeNodeFault::MixedInstancesAndValues))
        );
        pretty_assertions::assert_eq!(
            validate_attribute_node("speed", &json!([{ "type": "Feature" }, 30])),
            Err(refusal(&[PathSegment::Member("speed"), PathSegment::Index(0)], not_an_attribute_type()))
        );
    }

    /// A value is data: a `type` member inside a Property's `value`, or inside
    /// an object that is itself an implicit Property value, declares nothing.
    #[test]
    fn a_type_member_inside_a_value_is_data() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "declared": { "type": "Property", "value": { "type": "Feature", "x": 1 } },
            "implicit": { "a": { "type": "Feature", "x": 1 } },
            "values": [{ "a": { "type": "Feature" } }, 1]
        });
        assert!(validate_fragment_attribute_types(&fragment).is_ok());
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
                Err(refusal(&[PathSegment::Member("speed")], seal(member))),
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
                Err(refusal(&[PathSegment::Member("belongsTo")], seal(member))),
                "{member}"
            );
        }
    }

    /// Clause 4.5.3.3 repeats the same prohibition for the concise
    /// representation, where a Relationship declares no `type` at all. The
    /// family the rule needs comes from
    /// [`read_object`](crate::concise_reading::read_object), which reads the
    /// `object` member the clause makes the Relationship's value member -- so
    /// the seal rule is reached on a node that names no family of its own.
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
                Err(refusal(&[PathSegment::Member("belongsTo")], seal(member))),
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
                Err(refusal(&[PathSegment::Member("ngsildproof")], seal("entityIdSealed")))
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
            Err(refusal(
                &[PathSegment::Member("speed"), PathSegment::Member("providedBy")],
                seal("entityTypeSealed")
            ))
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
            Err(refusal(
                &[PathSegment::Member("speed"), PathSegment::Member("accuracy")],
                seal("entityIdSealed")
            ))
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
