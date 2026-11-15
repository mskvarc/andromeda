//! The clause 4.9 target value one attribute instance carries, read as
//! [`family_reading`] states for its family.

use crate::{
    q::target_value::{ResolvedTarget, TargetValue},
    queried_entity::{
        attribute_reading::{QueriedInstance, QueriedValueNode, ValuePresence},
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_query::filter::reading::family_reading::{TerminalKeyword, ValueLayout, family_reading};
use std::borrow::Cow;

/// The target value `item` carries under `keyword`, or `None` when it
/// carries none.
///
/// An item with nothing under the keyword holds nothing a Query Term
/// operand could be identical to or included in, so leaving it out changes
/// no comparison.
pub(crate) fn terminal_value<'a, N: QueriedValueNode<'a>>(keyword: TerminalKeyword, item: ValueItem<'a, N>) -> Option<TargetValue<'a>> {
    match (keyword, item) {
        (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Literal(literal)) => Some(TargetValue::literal(literal)),
        (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Json(json)) => Some(TargetValue::json(json)),
        (TerminalKeyword::Identifier, ValueItem::Reference(iri)) => Some(TargetValue::Text(Cow::Borrowed(iri))),
        (TerminalKeyword::Identifier, ValueItem::Node(node)) => node.id().map(|iri| TargetValue::Text(Cow::Borrowed(iri))),
        (TerminalKeyword::Literal, ValueItem::Reference(_) | ValueItem::Node(_))
        | (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Geometry(_) | ValueItem::Opaque) => None,
    }
}

/// The clause 4.9 target value an attribute instance's family carries, or
/// `None` when it holds none.
///
/// A structured Property value carries no literal of its own, and neither does
/// a geometry or a value no member reads: each is an Object target
/// ([`TargetValue::Structure`]), which no operand is identical to, so an
/// asserted test fails over it and its complement holds, as the SQL rendering
/// probes the instance whatever its value. A Property holding the empty array
/// holds an empty array target, which includes no operand, so `==` fails over
/// it and `!=` holds alike; an instance holding no value, a deletion among
/// them, has no target value. A path addressing a member of a structured value
/// descends into it instead. A Relationship's node without an object names
/// nothing and is no target.
#[must_use]
pub fn family_target<'a, I: QueriedInstance<'a>>(instance: I) -> Option<ResolvedTarget<'a>> {
    let reading = family_reading(instance.family());
    let terminals = instance.values().filter_map(|item| own_value(reading.terminal, item));
    let value = match reading.layout {
        ValueLayout::Nodes => match (held_values_target(terminals), reading.terminal, instance.value_presence()) {
            (Some(value), TerminalKeyword::Literal | TerminalKeyword::Identifier, ValuePresence::Held | ValuePresence::Absent) => value,
            (None, TerminalKeyword::Literal, ValuePresence::Held) => TargetValue::Array(Vec::new()),
            (None, TerminalKeyword::Literal, ValuePresence::Absent) | (None, TerminalKeyword::Identifier, ValuePresence::Held | ValuePresence::Absent) => {
                return None;
            }
        },
        ValueLayout::OrderedList => TargetValue::Array(terminals.collect()),
    };
    Some(ResolvedTarget {
        value,
        origin: reading.origin,
        reading: reading.reading,
    })
}

/// The target value one item of an instance's own value carries under
/// `keyword`: its literal, or an Object target for a literal-keyed item that
/// holds none.
fn own_value<'a, N: QueriedValueNode<'a>>(keyword: TerminalKeyword, item: ValueItem<'a, N>) -> Option<TargetValue<'a>> {
    match (keyword, &item) {
        (TerminalKeyword::Literal, ValueItem::Reference(_) | ValueItem::Node(_) | ValueItem::Geometry(_) | ValueItem::Opaque) => Some(TargetValue::Structure),
        (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Literal(_) | ValueItem::Json(_))
        | (TerminalKeyword::Identifier, ValueItem::Reference(_) | ValueItem::Node(_) | ValueItem::Geometry(_) | ValueItem::Opaque) => {
            terminal_value(keyword, item)
        }
    }
}

/// The target value a member holding one value node per value the client
/// wrote carries: a single value is a scalar target, several are an array
/// target, and none is no target.
pub(crate) fn held_values_target<'a>(mut values: impl Iterator<Item = TargetValue<'a>>) -> Option<TargetValue<'a>> {
    let first = values.next()?;
    Some(match values.next() {
        None => first,
        Some(second) => TargetValue::Array([first, second].into_iter().chain(values).collect()),
    })
}

/// The first value of an attribute instance's family member, which is the node
/// a compound value's members hang off.
///
/// A list family holds its items inside one list, which has neither a literal
/// nor members of its own, so a path into it addresses nothing.
#[must_use]
pub fn first_family_value<'a, I: QueriedInstance<'a>>(instance: I) -> Option<ValueItem<'a, I::Node>> {
    match family_reading(instance.family()).layout {
        ValueLayout::Nodes => instance.values().next(),
        ValueLayout::OrderedList => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        q::{family_target::family_target, target_value::TargetValue},
        queried_entity::expanded_fixture::{FixtureInstance, target_json},
    };
    use andromeda_ngsi_ld_query::filter::reading::family_reading::TargetOrigin;
    use serde_json::{Value, json};

    /// The target value one expanded attribute instance carries, panicking when
    /// it carries none.
    fn target_of(instance: &Value) -> (Value, TargetOrigin) {
        let instance = FixtureInstance::from_expanded(instance);
        let target = family_target(&instance).expect("a target value");
        (target_json(&target.value), target.origin)
    }

    /// A Property holding a literal beside a structured value holds two
    /// values, so its target is an array whatever its values are: the node is
    /// an Object element no operand is included as, and the literal one element
    /// beside it, as the stored array holds them.
    #[test]
    fn a_literal_beside_a_node_is_an_array_target() {
        let (value, origin) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "x"}, {"https://example.com/a": [{"@value": 1}]}]
        }));
        pretty_assertions::assert_eq!(value, json!(["x", {}]));
        assert_eq!(origin, TargetOrigin::AttributeValue);
    }

    /// Clause 4.9: "If the target element is a `ListProperty`, the target value
    /// is defined as the valueList array associated to such a `ListProperty`."
    #[test]
    fn a_list_property_resolves_to_the_whole_value_list() {
        let (value, origin) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}, {"@value": "blue"}]}]
        }));
        pretty_assertions::assert_eq!(value, json!(["red", "blue"]));
        assert_eq!(origin, TargetOrigin::AttributeValue);
    }

    /// Clause 4.9: "If the target element is a `ListRelationship`, the target
    /// object is defined as the array of objects associated (represented as
    /// URIs) to such `ListRelationship`."
    ///
    /// The core context declares `objectList` with `@container: "@list"` and no
    /// `@type: "@id"` coercion, so its members expand to `@value` nodes; a
    /// context that does coerce, and every document written in expanded form,
    /// gives `@id` nodes. Both spell the same target object.
    #[test]
    fn a_list_relationship_resolves_to_the_whole_object_list_in_either_expansion() {
        // The third member shape is the normalized representation of clause 4.5.22.2: a Relationship Object wrapper carrying `hasObject`.
        for member in [
            json!({"@value": "urn:ngsi-ld:Part:1"}),
            json!({"@id": "urn:ngsi-ld:Part:1"}),
            json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Part:1"}]}),
            json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@value": "urn:ngsi-ld:Part:1"}]}),
        ] {
            let (value, origin) = target_of(&json!({
                "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [member, {"@value": "urn:ngsi-ld:Part:2"}]}]
            }));
            pretty_assertions::assert_eq!(value, json!(["urn:ngsi-ld:Part:1", "urn:ngsi-ld:Part:2"]));
            assert_eq!(origin, TargetOrigin::RelationshipObject, "a list of objects is a target object");
        }
    }

    /// A single-element `valueList` is still the array clause 4.9 names, which
    /// is what keeps the ordering and pattern operators (defined over a scalar
    /// and a String target) off it.
    #[test]
    fn a_single_element_value_list_is_still_an_array_target() {
        let (value, _) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}]}]
        }));
        pretty_assertions::assert_eq!(value, json!(["red"]));
    }

    /// Clause 4.9's `equal`: "Is included in the target value, if the latter is
    /// an array". A Property whose value the client wrote as an array expands to
    /// one `hasValue` node per element, and all of them are the target value.
    #[test]
    fn an_array_valued_property_resolves_to_every_element() {
        let (value, _) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "a"}, {"@value": "b"}, {"@value": "c"}]
        }));
        pretty_assertions::assert_eq!(value, json!(["a", "b", "c"]));
    }

    /// Clause 4.9: the empty array includes no operand, so a Property holding it
    /// holds an empty array target, which no `==` term matches and every `!=`
    /// term does.
    #[test]
    fn an_empty_array_valued_property_resolves_an_empty_array() {
        let (value, origin) = target_of(&json!({"https://uri.etsi.org/ngsi-ld/hasValue": []}));
        pretty_assertions::assert_eq!(value, json!([]));
        assert_eq!(origin, TargetOrigin::AttributeValue);
    }

    /// A scalar Property value stays a scalar: only a client-written array
    /// expands to several value nodes.
    #[test]
    fn a_scalar_property_resolves_to_the_bare_value() {
        let (value, _) = target_of(&json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}));
        pretty_assertions::assert_eq!(value, json!(20));
    }

    /// Clause 4.9: "If the target element is a `VocabProperty`, the target value
    /// shall be expanded according to the @context", and the `equal` operator
    /// matches it when the operand "is included in the target value, if the
    /// latter is an array", so every term is a target, not only the first.
    #[test]
    fn a_multi_term_vocab_property_resolves_to_every_term() {
        let (value, origin) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasVocab": [{"@id": "https://example.com/Male"}, {"@id": "https://example.com/Other"}]
        }));
        pretty_assertions::assert_eq!(value, json!(["https://example.com/Male", "https://example.com/Other"]));
        assert_eq!(origin, TargetOrigin::AttributeValue, "a VocabProperty is not a Relationship");
    }

    /// Clause 4.9: "If the target element is a `LanguageProperty`, and no target
    /// language is specified, the target value is defined as a value from any of
    /// the key-value pairs held within the languageMap."
    #[test]
    fn a_language_property_with_no_target_language_resolves_to_every_language() {
        let (value, _) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                {"@language": "en", "@value": "Door"},
                {"@language": "de", "@value": "Tuer"}
            ]
        }));
        pretty_assertions::assert_eq!(value, json!(["Door", "Tuer"]));
    }

    /// Clause 4.9: "If the target element is a Relationship, the target object
    /// is defined as the object associated (represented as a URI or array of
    /// URIs) to such Relationship."
    #[test]
    fn a_relationship_resolves_to_its_target_object() {
        let (value, origin) = target_of(&json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Parking:1"}]}));
        pretty_assertions::assert_eq!(value, json!("urn:ngsi-ld:Parking:1"));
        assert_eq!(origin, TargetOrigin::RelationshipObject);
    }

    /// A Relationship naming several objects (clause 4.5.3) targets all of
    /// them, as an array of URIs.
    #[test]
    fn a_multi_target_relationship_resolves_to_every_object() {
        let (value, origin) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Parking:1"}, {"@id": "urn:ngsi-ld:Parking:2"}]
        }));
        pretty_assertions::assert_eq!(value, json!(["urn:ngsi-ld:Parking:1", "urn:ngsi-ld:Parking:2"]));
        assert_eq!(origin, TargetOrigin::RelationshipObject);
    }

    /// A `JsonProperty` value is raw JSON (clause 4.5.24); an array the client
    /// wrote there is an array target whose elements are compared by
    /// membership, exactly as a Property's array is.
    #[test]
    fn a_json_property_array_resolves_to_its_elements() {
        let (value, _) = target_of(&json!({"https://uri.etsi.org/ngsi-ld/hasJSON": [{"@value": [1, {"a": 2}], "@type": "@json"}]}));
        pretty_assertions::assert_eq!(value, json!([1, {"a": 2}]));
    }

    /// An expanded structured Property value carries no `@value` of its own, so
    /// it is an Object target no operand is identical to; a path descends into
    /// its member nodes instead.
    #[test]
    fn a_structured_property_value_is_an_object_target() {
        let instance = FixtureInstance::from_expanded(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.com/city": [{"@value": "Berlin"}]}]
        }));
        assert!(matches!(family_target(&instance).map(|target| target.value), Some(TargetValue::Structure)));
    }

    /// A `GeoProperty` held as a geometry carries no literal clause 4.9 could
    /// compare, so it is an Object target, while the clause 4.7.2 string form
    /// is a String like any other.
    #[test]
    fn a_geometry_is_an_object_target_and_its_string_form_is_text() {
        let geometry = FixtureInstance::from_expanded(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1}, {"@value": 2}]}]
            }]
        }));
        assert!(matches!(family_target(&geometry).map(|target| target.value), Some(TargetValue::Structure)));
        let (value, _) = target_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}]
        }));
        pretty_assertions::assert_eq!(value, json!("{\"type\":\"Point\",\"coordinates\":[1,2]}"));
    }
}
