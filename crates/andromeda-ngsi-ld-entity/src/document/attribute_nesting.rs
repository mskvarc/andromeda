//! How deeply one written attribute instance nests below itself.
//!
//! The nesting is counted on the document the expansion produced, in the
//! levels a client wrote: each sub-attribute, each node of a structured value
//! (a JSON object), each list (`valueList`, `objectList`, a `@list`), and each
//! object or array inside a JSON literal is one level, and a geometry takes the
//! levels of its `GeoJSON` node and its nested coordinate lists. Nested arrays a
//! Property value is written with are flattened by the expansion and stored as
//! one array, so they are no level. Each level costs at most two containers in
//! the stored expansion, which is what lets
//! [`AttributeNestingLimit::DECODABLE_CEILING`](andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit::DECODABLE_CEILING)
//! bound every stored document from the levels alone.

use crate::{
    document::{
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    list_object::ListObject,
    value_mode::ValueMode,
};
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNesting;

impl InstanceDocument {
    /// How deeply the instance nests below itself: through its value, and
    /// through each sub-attribute, which is a level of its own.
    #[must_use]
    pub fn nesting(&self) -> AttributeNesting {
        AttributeNesting::new(instance_levels(self))
    }
}

/// The levels below one instance.
fn instance_levels(instance: &InstanceDocument) -> usize {
    let value = match instance.value() {
        ValueSlot::Set(value) => written_value_levels(value),
        ValueSlot::Absent | ValueSlot::Remove(_) => 0,
    };
    let sub_attributes = instance
        .sub_attributes()
        .values()
        .flat_map(AttributeMember::instances)
        .map(|sub_attribute| instance_levels(sub_attribute).saturating_add(1))
        .max()
        .unwrap_or(0);
    value.max(sub_attributes)
}

/// The levels a written value takes below the instance carrying it.
fn written_value_levels(value: &WrittenValue) -> usize {
    match value {
        WrittenValue::Value(values) => deepest(values),
        WrittenValue::ValueList(items) => deepest(items).saturating_add(1),
        WrittenValue::Geometry(geometry) => geometry_levels(geometry),
        WrittenValue::Json(literals) => literals.as_slice().iter().map(|literal| literal.piece().levels()).max().unwrap_or(0),
        WrittenValue::ObjectList(objects) => {
            // A wrapped item is a node holding its target, one level inside
            // the list.
            let wrapped = objects.iter().any(|object| matches!(object, ListObject::Wrapped(_)));
            usize::from(wrapped).saturating_add(1)
        }
        WrittenValue::Object(_) | WrittenValue::LanguageMap(_) | WrittenValue::Vocab(_) => 0,
    }
}

/// The levels of the deepest of `values`.
fn deepest<M: ValueMode>(values: &[ExpandedValue<M>]) -> usize {
    values.iter().map(value_levels).max().unwrap_or(0)
}

/// The levels one expanded value takes.
fn value_levels<M: ValueMode>(value: &ExpandedValue<M>) -> usize {
    match value {
        ExpandedValue::Literal(_) => 0,
        ExpandedValue::Json(literal) => literal.piece().levels(),
        ExpandedValue::List(items) => deepest(items).saturating_add(1),
        ExpandedValue::Node(node) => node
            .members()
            .values()
            .map(|member| deepest(M::held_values(member)))
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    }
}

/// The levels of a geometry: its node, and below it the deepest of its
/// coordinate lists, its `bbox` list and its foreign members. A geometry
/// written as a JSON string (clause 4.7.2) is stored as that string.
fn geometry_levels(geometry: &GeoValue) -> usize {
    match geometry {
        GeoValue::Encoded(_) => 0,
        GeoValue::Geometry(value) => {
            let coordinates = value.form().geometry_type().coordinate_nesting();
            let members = value.members().map_or(0, |members| {
                let bbox = usize::from(members.bbox().is_some());
                members.foreign().values().map(|values| deepest(values)).max().unwrap_or(0).max(bbox)
            });
            coordinates.max(members).saturating_add(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_test_fixtures::nesting_fixture::{NESTED_ATTRIBUTE, NestingShape, nested_instance, node_chain};
    use serde_json::{Map, Value, json};

    const GEOJSON: &str = "https://purl.org/geojson/vocab#";

    /// The expanded IRI of an NGSI-LD core term.
    fn core(term: &str) -> String {
        format!("https://uri.etsi.org/ngsi-ld/{term}")
    }

    /// The nesting of the written instance `expanded` reads as.
    fn nesting(expanded: &Value) -> usize {
        let attribute: AttributeName = NESTED_ATTRIBUTE.parse().expect("an attribute name");
        InstanceDocument::from_slice(&attribute, &serde_json::to_vec(expanded).expect("JSON"))
            .expect("a written instance")
            .nesting()
            .get()
    }

    /// An instance of `family` carrying `member` under the core term `carrier`.
    fn instance(family: &str, carrier: &str, member: Value) -> Value {
        Value::Object(Map::from_iter([("@type".to_owned(), json!([core(family)])), (core(carrier), member)]))
    }

    /// One position as a `@list` of two ordinates.
    fn position(x: f64, y: f64) -> Value {
        json!({"@list": [{"@value": x}, {"@value": y}]})
    }

    /// A closed ring of four positions.
    fn ring() -> Value {
        json!({"@list": [position(0.0, 0.0), position(1.0, 0.0), position(1.0, 1.0), position(0.0, 0.0)]})
    }

    /// A `GeoProperty` of the geometry `kind` with `coordinates` and `members`.
    fn geometry(kind: &str, coordinates: Value, members: Vec<(&str, Value)>) -> Value {
        let mut node = Map::from_iter([
            ("@type".to_owned(), json!([format!("{GEOJSON}{kind}")])),
            (format!("{GEOJSON}coordinates"), Value::Array(vec![coordinates])),
        ]);
        node.extend(members.into_iter().map(|(name, value)| (name.to_owned(), value)));
        instance("GeoProperty", "hasValue", Value::Array(vec![Value::Object(node)]))
    }

    #[test]
    fn a_scalar_property_nests_nothing() {
        pretty_assertions::assert_eq!(nesting(&instance("Property", "hasValue", json!([{"@value": 1}]))), 0);
    }

    /// Each node of a structured value is one level.
    #[test]
    fn a_chain_of_nodes_nests_one_level_per_node() {
        for levels in [1, 5, 59] {
            pretty_assertions::assert_eq!(nesting(&nested_instance(NestingShape::ObjectChain, levels)), levels);
        }
    }

    /// Nested arrays of a Property value are flattened by the expansion, and
    /// a value of several items nests as deeply as its deepest item.
    #[test]
    fn the_items_of_a_value_nest_as_deeply_as_the_deepest() {
        let flattened = instance("Property", "hasValue", json!([{"@value": 1}, {"@value": 2}, {"@value": 3}]));
        pretty_assertions::assert_eq!(nesting(&flattened), 0);
        let items = instance("Property", "hasValue", json!([node_chain(1), node_chain(2)]));
        pretty_assertions::assert_eq!(nesting(&items), 2);
    }

    #[test]
    fn a_value_list_is_a_level_and_each_list_in_it_another() {
        let flat = instance("ListProperty", "hasValueList", json!([{"@list": [{"@value": 1}, {"@value": 2}]}]));
        pretty_assertions::assert_eq!(nesting(&flat), 1);
        for levels in [2, 33, 59] {
            pretty_assertions::assert_eq!(nesting(&nested_instance(NestingShape::ListChain, levels)), levels);
        }
    }

    /// Clause 4.5.24 keeps a JSON literal verbatim; each of its containers is
    /// a level.
    #[test]
    fn each_container_of_a_json_literal_is_a_level() {
        for levels in [0, 2, 59] {
            pretty_assertions::assert_eq!(nesting(&nested_instance(NestingShape::JsonChain, levels)), levels);
        }
        let arrays = instance("JsonProperty", "hasJSON", json!([{"@type": "@json", "@value": [[1]]}]));
        pretty_assertions::assert_eq!(nesting(&arrays), 2);
    }

    /// Clause 4.7.1 excludes `GeometryCollection`, so a geometry is its node
    /// and the coordinate lists its type fixes, or its `bbox` and foreign
    /// members when they nest deeper.
    #[test]
    fn a_geometry_nests_its_node_and_its_coordinate_lists() {
        let point = position(1.0, 2.0);
        let line = json!({"@list": [position(0.0, 0.0), position(1.0, 1.0)]});
        pretty_assertions::assert_eq!(nesting(&geometry("Point", point.clone(), Vec::new())), 2);
        pretty_assertions::assert_eq!(nesting(&geometry("LineString", line, Vec::new())), 3);
        pretty_assertions::assert_eq!(nesting(&geometry("Polygon", json!({"@list": [ring()]}), Vec::new())), 4);
        pretty_assertions::assert_eq!(nesting(&geometry("MultiPolygon", json!({"@list": [{"@list": [ring()]}]}), Vec::new())), 5);
        let bbox = json!([{"@list": [{"@value": 0.0}, {"@value": 0.0}, {"@value": 1.0}, {"@value": 1.0}]}]);
        pretty_assertions::assert_eq!(
            nesting(&geometry("Point", point.clone(), vec![("https://purl.org/geojson/vocab#bbox", bbox)])),
            2
        );
        let foreign = json!([node_chain(3)]);
        pretty_assertions::assert_eq!(nesting(&geometry("Point", point, vec![("https://example.org/title", foreign)])), 4);
    }

    #[test]
    fn targets_terms_and_language_maps_nest_nothing_and_an_object_list_one_level() {
        pretty_assertions::assert_eq!(nesting(&instance("Relationship", "hasObject", json!([{"@id": "urn:ngsi-ld:T:1"}]))), 0);
        pretty_assertions::assert_eq!(nesting(&instance("VocabProperty", "hasVocab", json!([{"@id": "https://example.org/a"}]))), 0);
        pretty_assertions::assert_eq!(
            nesting(&instance("LanguageProperty", "hasLanguageMap", json!([{"@value": "x", "@language": "en"}]))),
            0
        );
        let bare = instance("ListRelationship", "hasObjectList", json!([{"@list": [{"@id": "urn:ngsi-ld:T:1"}]}]));
        pretty_assertions::assert_eq!(nesting(&bare), 1);
        let wrapped = instance(
            "ListRelationship",
            "hasObjectList",
            json!([{"@list": [{core("hasObject"): [{"@id": "urn:ngsi-ld:T:1"}]}]}]),
        );
        pretty_assertions::assert_eq!(nesting(&wrapped), 2);
    }

    /// Clause 4.5.2: a sub-attribute is a level, and its value nests below it.
    #[test]
    fn a_sub_attribute_is_a_level_above_its_own_value() {
        let mut scalar = instance("Property", "hasValue", json!([{"@value": 1}]));
        let mut structured = scalar.clone();
        if let (Some(scalar), Some(structured)) = (scalar.as_object_mut(), structured.as_object_mut()) {
            scalar.insert(
                "https://example.org/accuracy".to_owned(),
                json!([instance("Property", "hasValue", json!([{"@value": 1}]))]),
            );
            structured.insert(
                "https://example.org/accuracy".to_owned(),
                json!([instance("Property", "hasValue", json!([node_chain(1)]))]),
            );
        }
        pretty_assertions::assert_eq!(nesting(&scalar), 1);
        pretty_assertions::assert_eq!(nesting(&structured), 2);
        for levels in [1, 32, 59] {
            pretty_assertions::assert_eq!(nesting(&nested_instance(NestingShape::SubAttributeChain, levels)), levels);
        }
    }

    /// A member coerced to `@json` or to a `@list` by the client's context is
    /// a level below the node holding it, and each list inside a list another.
    #[test]
    fn a_json_or_list_member_of_a_node_nests_below_the_node() {
        let json_member = instance(
            "Property",
            "hasValue",
            json!([{"https://example.org/raw": [{"@type": "@json", "@value": {"a": 1}}]}]),
        );
        pretty_assertions::assert_eq!(nesting(&json_member), 2);
        let list_member = instance(
            "Property",
            "hasValue",
            json!([{"https://example.org/floors": [{"@list": [{"@list": [{"@value": 1}]}]}]}]),
        );
        pretty_assertions::assert_eq!(nesting(&list_member), 3);
    }

    /// The deepest path decides, whether it runs through the value or a
    /// sub-attribute.
    #[test]
    fn the_deepest_of_the_value_and_the_sub_attributes_decides() {
        let mut written = instance("Property", "hasValue", json!([node_chain(5)]));
        if let Some(members) = written.as_object_mut() {
            members.insert("https://example.org/a".to_owned(), json!([nested_instance(NestingShape::ObjectChain, 3)]));
            members.insert("https://example.org/b".to_owned(), json!([nested_instance(NestingShape::ObjectChain, 7)]));
        }
        pretty_assertions::assert_eq!(nesting(&written), 8);
    }
}
