//! Rejecting attribute nodes whose `type` is not an NGSI-LD attribute type.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4: data that is not syntactically correct
//! according to the API data type definitions is an error of type
//! `BadRequestData`. Clause 5.2.4 fixes the attribute types to the eight of
//! [`AttributeType`], so a node that declares any other token -- `"Propertyy"`,
//! `"NotAnAttributeType"` -- is not an attribute and must not reach the store
//! as one. The check runs on the request's compact payload, before the
//! normalization of clause 4.5.2.3 infers a type where none was declared.

use crate::entity_members::ENTITY_MEMBERS;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde_json::{Map, Value};
use std::str::FromStr;
use thiserror::Error;

/// An attribute node declaring a `type` outside clause 5.2.4.
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

/// Whether an instance's declared `type`, if it is a string, names one of the
/// eight attribute types. A `type` that is not a string is the `type` of a
/// concise value object, not a declaration, and is left alone.
fn validate_instance(attribute: &str, instance: &Map<String, Value>) -> Result<(), AttributeTypeError> {
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

/// The RFC 7946 geometry tokens a bare GeoJSON geometry value declares as its
/// `type` (clause 4.5.2.3 geospatial exception): such an object is the value
/// of a GeoProperty, not an attribute node, and is not a declaration to check.
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

    #[test]
    fn a_single_attribute_fragment_is_checked_as_the_node_itself() {
        assert!(validate_attribute_node("speed", &json!({ "value": 99 })).is_ok());
        assert!(validate_attribute_node("speed", &json!({ "type": "Relationship", "object": "urn:ngsi-ld:X:1" })).is_ok());
        assert!(validate_attribute_node("speed", &json!({ "type": "Propertyy", "value": 1 })).is_err());
    }
}
