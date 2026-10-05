//! What a compact node written where an attribute stands is (ETSI GS CIM 009
//! v1.9.1 clause 4.5.2.3 and its siblings).
//!
//! An attribute, or a sub-attribute, may be written normalized, concisely, or
//! as a bare value, and which of the three a node is decides what it means.
//! The reading is stated here once, because three parties have to agree on it
//! exactly: the validation that refuses a node the clauses do not admit, the
//! rewrite into the normalized representation that reads the admitted ones,
//! and the concise rendering, which may only write a node that reads back as
//! the attribute it renders.
//!
//! An object reads as:
//!
//! - an instance of the attribute type its `type` member names, since clause
//!   4.5.2.3 treats an object carrying a `type` as a normalized attribute; a
//!   `type` naming a `GeoJSON` geometry beside its coordinates is instead the
//!   bare geometry the same clause's geospatial exception makes a
//!   `GeoProperty`'s value, and any other `type` is refused;
//! - without a `type`, an instance of the family whose value member it carries
//!   (clauses 4.5.3.3, 4.5.18.3, 4.5.20.3, 4.5.21.3, 4.5.22.3, 4.5.24.3), or,
//!   carrying `value` alone of them, a Property -- a `GeoProperty` when that
//!   value is a geometry -- since clause 4.5.2.3 infers `Property` "by the
//!   presence of the "value" attribute";
//! - and otherwise the value of an implicit Property.
//!
//! An array reads as the instances of a multi-attribute when every element is
//! an instance (clause 4.5.5), as the value of an implicit Property when no
//! element is, and is refused when it mixes the two.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape};
use andromeda_ngsi_ld_geometry::geometry_type::GeometryType;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use std::str::FromStr;

/// The member an attribute names its type in, and a geometry its kind.
const TYPE_MEMBER: &str = "type";

/// The geometry type IETF RFC 7946 section 3.1.8 defines beside the six
/// [`GeometryType`] holds. Clause 4.7.1 admits no `GeoProperty` of it, and
/// reading one as a geometry is what routes it to the geometry validation that
/// refuses it, rather than letting it pass as an ordinary Property value.
const GEOMETRY_COLLECTION: &str = "GeometryCollection";

/// The member a `GeometryCollection` carries its geometries in (RFC 7946
/// section 3.1.8).
const GEOMETRIES_MEMBER: &str = "geometries";

/// The member every other geometry carries its coordinates in (RFC 7946
/// section 3.1.1).
const COORDINATES_MEMBER: &str = "coordinates";

/// What one object written where an attribute stands reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectReading {
    /// An instance of the attribute type it declares or implies.
    Instance(AttributeType),
    /// A bare `GeoJSON` geometry: the value of a `GeoProperty`.
    GeoPropertyValue,
    /// The value of an implicit Property.
    PropertyValue,
}

/// Why an object written where an attribute stands is no node the clauses
/// admit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRefusal {
    /// Its `type` names neither an attribute type (clause 5.2.4) nor a
    /// geometry.
    NotAnAttributeType(strum::ParseError),
    /// Its `type` is not a string.
    TypeNotAString,
    /// Its `type` names a geometry, but the member holding the geometry's
    /// coordinates is missing.
    GeometryWithoutCoordinates,
    /// It declares no `type` and carries the value members of several
    /// families.
    SeveralValueMembers,
}

/// What an array written where an attribute stands reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayReading {
    /// The instances of a multi-attribute.
    Instances,
    /// The value of an implicit Property.
    PropertyValue,
}

/// Why an array written where an attribute stands is no node the clauses
/// admit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayRefusal {
    /// One of its elements is refused.
    Element {
        /// Where the element stands in the array.
        index: usize,
        /// Why it is refused.
        refusal: ObjectRefusal,
    },
    /// Some of its elements are instances and some are not, so it is neither
    /// the instances of a multi-attribute nor one value.
    MixedInstancesAndValues,
}

/// Read one object written where an attribute stands.
///
/// # Errors
///
/// Returns the [`ObjectRefusal`] of an object the clauses do not admit there.
pub fn read_object<'n, N: CompactNode<'n>>(object: &NodeMembers<'n, N>) -> Result<ObjectReading, ObjectRefusal> {
    match object.get(TYPE_MEMBER).map(CompactNode::shape) {
        Some(NodeShape::String(token)) => read_declared(object, &token),
        Some(NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_)) => {
            Err(ObjectRefusal::TypeNotAString)
        }
        None => read_typeless(object),
    }
}

/// Read one array written where an attribute stands.
///
/// # Errors
///
/// Returns the [`ArrayRefusal`] of an array holding a refused element or
/// mixing instances with values.
pub fn read_array<'n, N: CompactNode<'n>>(items: &[N]) -> Result<ArrayReading, ArrayRefusal> {
    let instances = items.iter().enumerate().try_fold(0_usize, |instances, (index, item)| {
        is_instance(*item)
            .map(|instance| instances + usize::from(instance))
            .map_err(|refusal| ArrayRefusal::Element { index, refusal })
    })?;
    match instances {
        0 => Ok(ArrayReading::PropertyValue),
        all if all == items.len() => Ok(ArrayReading::Instances),
        _ => Err(ArrayRefusal::MixedInstancesAndValues),
    }
}

/// The family an object declaring no `type` implies through the value member
/// of one of the six families other than Property and `GeoProperty`, or
/// `None` when it carries none of them.
///
/// # Errors
///
/// Returns [`ObjectRefusal::SeveralValueMembers`] for an object carrying the
/// value members of two families.
pub fn value_member_family<'n, N: CompactNode<'n>>(object: &NodeMembers<'n, N>) -> Result<Option<AttributeType>, ObjectRefusal> {
    AttributeType::ALL
        .into_iter()
        .filter(|family| family.compact_value_key() != compact_properties::VALUE && object.contains(family.compact_value_key()))
        .try_fold(None, |found, family| match found {
            None => Ok(Some(family)),
            Some(_) => Err(ObjectRefusal::SeveralValueMembers),
        })
}

/// The family an instance declaring no `type` and carrying `value` is: a
/// `GeoProperty` when the value resolves to a `GeoJSON` geometry, a Property
/// otherwise (clause 4.5.2.3, "type").
#[must_use]
pub fn typeless_value_family<'n, N: CompactNode<'n>>(value: N) -> AttributeType {
    match value.shape() {
        NodeShape::Geometry(_) => AttributeType::GeoProperty,
        NodeShape::Object(members) if is_geometry(&members) => AttributeType::GeoProperty,
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) => {
            AttributeType::Property
        }
    }
}

/// The family a value written bare, with nothing around it, reads back as the
/// value of: a Property or a `GeoProperty`, or `None` when it reads as
/// something else or is refused.
#[must_use]
pub fn bare_value_family<'n, N: CompactNode<'n>>(value: N) -> Option<AttributeType> {
    match value.shape() {
        NodeShape::Object(object) => match read_object(&object) {
            Ok(ObjectReading::PropertyValue) => Some(AttributeType::Property),
            Ok(ObjectReading::GeoPropertyValue) => Some(AttributeType::GeoProperty),
            Ok(ObjectReading::Instance(_)) | Err(_) => None,
        },
        NodeShape::Array(items) => match read_array(&items) {
            Ok(ArrayReading::PropertyValue) => Some(AttributeType::Property),
            Ok(ArrayReading::Instances) | Err(_) => None,
        },
        NodeShape::Geometry(_) => Some(AttributeType::GeoProperty),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) => Some(AttributeType::Property),
    }
}

/// Read an object whose `type` member is the string `token`.
fn read_declared<'n, N: CompactNode<'n>>(object: &NodeMembers<'n, N>, token: &str) -> Result<ObjectReading, ObjectRefusal> {
    match (AttributeType::from_str(token), coordinates_member(token)) {
        (Ok(family), _) => Ok(ObjectReading::Instance(family)),
        (Err(_), Some(member)) if object.contains(member) => Ok(ObjectReading::GeoPropertyValue),
        (Err(_), Some(_)) => Err(ObjectRefusal::GeometryWithoutCoordinates),
        (Err(source), None) => Err(ObjectRefusal::NotAnAttributeType(source)),
    }
}

/// Read an object declaring no `type`.
fn read_typeless<'n, N: CompactNode<'n>>(object: &NodeMembers<'n, N>) -> Result<ObjectReading, ObjectRefusal> {
    Ok(match value_member_family(object)? {
        Some(family) => ObjectReading::Instance(family),
        None => object
            .get(compact_properties::VALUE)
            .map_or(ObjectReading::PropertyValue, |value| ObjectReading::Instance(typeless_value_family(value))),
    })
}

/// Whether one element of an array reads as an attribute instance.
fn is_instance<'n, N: CompactNode<'n>>(item: N) -> Result<bool, ObjectRefusal> {
    match item.shape() {
        NodeShape::Object(object) => read_object(&object).map(|reading| matches!(reading, ObjectReading::Instance(_))),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Geometry(_) => Ok(false),
    }
}

/// Whether an object is a `GeoJSON` geometry: a geometry `type` beside the
/// member holding its coordinates.
fn is_geometry<'n, N: CompactNode<'n>>(object: &NodeMembers<'n, N>) -> bool {
    object
        .get(TYPE_MEMBER)
        .and_then(CompactNode::text)
        .and_then(|token| coordinates_member(&token))
        .is_some_and(|member| object.contains(member))
}

/// The member a geometry whose `type` is `token` holds its coordinates in, or
/// `None` when `token` names no `GeoJSON` geometry type.
fn coordinates_member(token: &str) -> Option<&'static str> {
    if token == GEOMETRY_COLLECTION {
        return Some(GEOMETRIES_MEMBER);
    }
    GeometryType::from_str(token).ok().map(|_| COORDINATES_MEMBER)
}

#[cfg(test)]
mod tests {
    use crate::{
        compact_node::{CompactNode, NodeMembers, NodeShape},
        concise_reading::{ArrayReading, ArrayRefusal, ObjectReading, ObjectRefusal, bare_value_family, read_array, read_object, typeless_value_family},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::{Value, json};

    /// The members of the object `value` is.
    fn object(value: &Value) -> NodeMembers<'_, &Value> {
        match value.shape() {
            NodeShape::Object(members) => members,
            other @ (NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Geometry(_)) => {
                panic!("an object: {other:?}")
            }
        }
    }

    /// The elements of the array `value` is.
    fn array(value: &Value) -> Vec<&Value> {
        value.as_array().unwrap_or_else(|| panic!("an array: {value}")).iter().collect()
    }

    /// The same reading from the text of `value`, read in place.
    fn read_from_text(value: &Value) -> Result<ObjectReading, ObjectRefusal> {
        let literal = JsonLiteral::admit(&value.to_string()).expect("the fixture is JSON");
        match literal.piece().shape() {
            NodeShape::Object(members) => read_object(&members),
            other @ (NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Geometry(_)) => {
                panic!("an object: {other:?}")
            }
        }
    }

    #[test]
    fn an_object_declaring_an_attribute_type_is_an_instance_of_it() {
        for family in AttributeType::ALL {
            let node = json!({"type": family.to_string(), family.compact_value_key(): 1});
            pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::Instance(family)), "{family}");
        }
    }

    #[test]
    fn a_bare_geometry_is_a_geo_property_value() {
        for node in [
            json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            json!({"type": "GeometryCollection", "geometries": []}),
        ] {
            pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::GeoPropertyValue), "{node}");
        }
    }

    /// Clause 4.7.1 and IETF RFC 7946 section 3.1: a geometry is its type
    /// together with its coordinates.
    #[test]
    fn a_geometry_type_without_its_coordinates_is_refused() {
        for node in [json!({"type": "Point"}), json!({"type": "GeometryCollection", "coordinates": []})] {
            pretty_assertions::assert_eq!(read_object(&object(&node)), Err(ObjectRefusal::GeometryWithoutCoordinates), "{node}");
        }
    }

    /// Clause 4.5.2.3 treats an object carrying a `type` as a normalized
    /// attribute, whose `type` clause 5.2.4 confines to eight tokens.
    #[test]
    fn a_type_naming_no_attribute_type_is_refused() {
        for node in [json!({"type": "Feature", "x": 1}), json!({"type": "Propertyy", "value": 1})] {
            pretty_assertions::assert_eq!(
                read_object(&object(&node)),
                Err(ObjectRefusal::NotAnAttributeType(strum::ParseError::VariantNotFound)),
                "{node}"
            );
        }
    }

    #[test]
    fn a_type_that_is_not_a_string_is_refused() {
        for node in [json!({"type": 7, "value": 1}), json!({"type": ["Property"], "value": 1})] {
            pretty_assertions::assert_eq!(read_object(&object(&node)), Err(ObjectRefusal::TypeNotAString), "{node}");
        }
    }

    #[test]
    fn a_typeless_object_with_one_value_member_is_that_family() {
        for (node, family) in [
            (json!({"object": "urn:ngsi-ld:X:1"}), AttributeType::Relationship),
            (json!({"languageMap": {"en": "x"}}), AttributeType::LanguageProperty),
            (json!({"vocab": "x"}), AttributeType::VocabProperty),
            (json!({"valueList": [1]}), AttributeType::ListProperty),
            (json!({"objectList": ["urn:ngsi-ld:X:1"]}), AttributeType::ListRelationship),
            (json!({"json": {"k": 1}}), AttributeType::JsonProperty),
        ] {
            pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::Instance(family)), "{node}");
        }
    }

    #[test]
    fn a_typeless_object_with_two_value_members_is_refused() {
        let node = json!({"object": "urn:ngsi-ld:X:1", "languageMap": {"en": "x"}});
        pretty_assertions::assert_eq!(read_object(&object(&node)), Err(ObjectRefusal::SeveralValueMembers));
    }

    /// Clause 4.5.2.3, "type": "If missing, "Property" can be inferred by the
    /// presence of the "value" attribute", with or without sub-attributes.
    #[test]
    fn a_typeless_object_with_value_is_a_property_instance() {
        for node in [json!({"value": 42}), json!({"value": 42, "accuracy": 0.5}), json!({"value": {"value": 5}})] {
            pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::Instance(AttributeType::Property)), "{node}");
        }
    }

    /// The exception the same clause makes for a geospatial value.
    #[test]
    fn a_typeless_object_whose_value_is_a_geometry_is_a_geo_property_instance() {
        let node = json!({"value": {"type": "Point", "coordinates": [1.0, 2.0]}});
        pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::Instance(AttributeType::GeoProperty)));
    }

    /// The other family's value member names the family; the `value` it then
    /// carries is refused for that family after expansion.
    #[test]
    fn a_typeless_object_with_value_and_another_value_member_is_that_family() {
        let node = json!({"value": 1, "object": "urn:ngsi-ld:X:1"});
        pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::Instance(AttributeType::Relationship)));
    }

    #[test]
    fn any_other_object_is_a_property_value() {
        for node in [json!({"a": 1}), json!({}), json!({"Type": "Point", "coordinates": [1.0, 2.0]})] {
            pretty_assertions::assert_eq!(read_object(&object(&node)), Ok(ObjectReading::PropertyValue), "{node}");
        }
    }

    #[test]
    fn an_array_of_instances_reads_as_instances() {
        let node = json!([{"value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 30}]);
        pretty_assertions::assert_eq!(read_array(&array(&node)), Ok(ArrayReading::Instances));
    }

    #[test]
    fn an_array_of_values_reads_as_a_value() {
        for node in [
            json!([1, 2]),
            json!([]),
            json!([{"a": 1}]),
            json!([{"type": "Point", "coordinates": [1.0, 2.0]}]),
        ] {
            pretty_assertions::assert_eq!(read_array(&array(&node)), Ok(ArrayReading::PropertyValue), "{node}");
        }
    }

    #[test]
    fn an_array_mixing_instances_and_values_is_refused() {
        let node = json!([{"value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"}, 30]);
        pretty_assertions::assert_eq!(read_array(&array(&node)), Err(ArrayRefusal::MixedInstancesAndValues));
    }

    #[test]
    fn a_refused_element_refuses_its_array() {
        let node = json!([1, {"type": "Feature"}]);
        pretty_assertions::assert_eq!(
            read_array(&array(&node)),
            Err(ArrayRefusal::Element {
                index: 1,
                refusal: ObjectRefusal::NotAnAttributeType(strum::ParseError::VariantNotFound),
            })
        );
    }

    #[test]
    fn bare_value_family_is_what_a_bare_value_reads_back_as() {
        for value in [json!(42), json!("text"), json!({"a": 1}), json!(["a"]), json!([])] {
            pretty_assertions::assert_eq!(bare_value_family(&value), Some(AttributeType::Property), "{value}");
        }
        pretty_assertions::assert_eq!(
            bare_value_family(&json!({"type": "Point", "coordinates": [1.0, 2.0]})),
            Some(AttributeType::GeoProperty)
        );
        for value in [
            json!({"type": "Feature"}),
            json!({"object": "urn:ngsi-ld:X:1"}),
            json!({"value": 1}),
            json!([{"object": "urn:ngsi-ld:X:1"}]),
            json!([{"value": 1}, 2]),
        ] {
            pretty_assertions::assert_eq!(bare_value_family(&value), None, "{value}");
        }
    }

    #[test]
    fn typeless_value_family_is_geo_property_only_for_a_geometry() {
        pretty_assertions::assert_eq!(
            typeless_value_family(&json!({"type": "Point", "coordinates": [1.0, 2.0]})),
            AttributeType::GeoProperty
        );
        for value in [json!(42), json!({"type": "Feature"}), json!(r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#)] {
            pretty_assertions::assert_eq!(typeless_value_family(&value), AttributeType::Property, "{value}");
        }
    }

    /// The reading of a request's text and of a parsed document agree.
    #[test]
    fn text_and_a_parsed_document_read_alike() {
        for node in [
            json!({"type": "Property", "value": 1}),
            json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            json!({"type": "Point"}),
            json!({"type": 7}),
            json!({"object": "urn:ngsi-ld:X:1", "vocab": "x"}),
            json!({"value": {"type": "Point", "coordinates": [1.0, 2.0]}}),
            json!({"a": 1}),
        ] {
            pretty_assertions::assert_eq!(read_from_text(&node), read_object(&object(&node)), "{node}");
        }
    }
}
