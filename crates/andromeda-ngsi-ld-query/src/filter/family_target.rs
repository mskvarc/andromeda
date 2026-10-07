//! The clause 4.9 target value each attribute family carries.
//!
//! Clause 4.9 defines a target value for every attribute family of clause
//! 5.2.4, and the definitions differ in shape, not only in which member carries
//! them:
//!
//! * "If the target element is a Property, the target value is defined as the
//!   Value associated to such Property" — one value, or, when the client wrote
//!   an array, the array JSON-LD expansion turned into several `hasValue`
//!   nodes;
//! * "If the target element is a `LanguageProperty`, and no target language is
//!   specified, the target value is defined as a value from any of the
//!   key-value pairs held within the languageMap";
//! * "If the target element is a `ListProperty`, the target value is defined as
//!   the valueList array associated to such a `ListProperty`";
//! * "If the target element is a `ListRelationship`, the target object is
//!   defined as the array of objects associated (represented as URIs) to such
//!   `ListRelationship`";
//! * "If the target element is a `VocabProperty`, the target value shall be
//!   expanded according to the @context";
//! * "If the target element is a Relationship, the target object is defined as
//!   the object associated (represented as a URI or array of URIs)".
//!
//! Every one of those that can hold more than one value resolves to an array, which is what makes the clause's array rules -- "Is included in the target value, if the latter is an array", and under `unequal` "Is not included in the target value, if the latter is an array" -- apply to it.
//!
//! The two list families are ordered, so their target is the array even when
//! it holds a single element, which is what keeps the ordering and pattern
//! operators off it.

use crate::{
    filter::target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue},
    queried_entity::{
        attribute_reading::{QueriedInstance, QueriedValueNode},
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use std::borrow::Cow;

/// How a family's value member holds its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueCarrier {
    /// One value node per value the client wrote. A single value is a scalar
    /// target; several are an array target.
    Nodes,
    /// One ordered list, whose items are the target array whatever their count.
    OrderedList,
}

/// Which values of a member a target is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalKeyword {
    /// Literals only, the values a JSON-LD value object carries under
    /// `@value`.
    Literal,
    /// Literals and the IRIs references and nodes are identified by, carried
    /// under `@id`: a Relationship's objects, a `VocabProperty`'s terms, and
    /// the end of a path that stops on a node.
    Identifier,
}

impl TerminalKeyword {
    /// The target value one item carries under this keyword, or `None` when it
    /// carries none.
    ///
    /// An item with nothing under the keyword holds nothing a Query Term
    /// operand could be identical to or included in, so leaving it out changes
    /// no comparison.
    pub(crate) fn read<'a, N: QueriedValueNode<'a>>(self, item: ValueItem<'a, N>) -> Option<TargetValue<'a>> {
        match (self, item) {
            (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Literal(literal)) => Some(TargetValue::literal(literal)),
            (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Json(json)) => Some(TargetValue::json(json)),
            (TerminalKeyword::Identifier, ValueItem::Reference(iri)) => Some(TargetValue::Text(Cow::Borrowed(iri))),
            (TerminalKeyword::Identifier, ValueItem::Node(node)) => node.id().map(|iri| TargetValue::Text(Cow::Borrowed(iri))),
            (TerminalKeyword::Literal, ValueItem::Reference(_) | ValueItem::Node(_))
            | (TerminalKeyword::Literal | TerminalKeyword::Identifier, ValueItem::Opaque) => None,
        }
    }
}

/// How clause 4.9 reads one attribute family's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FamilyReading {
    /// How the family's value member holds its values.
    carrier: ValueCarrier,
    /// Which values of the member make the target.
    terminal: TerminalKeyword,
    /// Which clause 4.9 definition the values satisfy.
    origin: TargetOrigin,
    /// What clause 4.9 makes of several values.
    reading: MultiValueReading,
}

/// How clause 4.9 reads the values of `family`, covering all eight attribute
/// families of clause 5.2.4 so a `q` over any of them resolves.
const fn family_reading(family: AttributeType) -> FamilyReading {
    let (carrier, terminal, origin, reading) = match family {
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::JsonProperty => (
            ValueCarrier::Nodes,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
        ),
        AttributeType::Relationship => (
            ValueCarrier::Nodes,
            TerminalKeyword::Identifier,
            TargetOrigin::RelationshipObject,
            MultiValueReading::WholeArray,
        ),
        AttributeType::VocabProperty => (
            ValueCarrier::Nodes,
            TerminalKeyword::Identifier,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
        ),
        AttributeType::LanguageProperty => (
            ValueCarrier::Nodes,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::AnyOneValue,
        ),
        AttributeType::ListProperty => (
            ValueCarrier::OrderedList,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
        ),
        AttributeType::ListRelationship => (
            ValueCarrier::OrderedList,
            TerminalKeyword::Identifier,
            TargetOrigin::RelationshipObject,
            MultiValueReading::WholeArray,
        ),
    };
    FamilyReading {
        carrier,
        terminal,
        origin,
        reading,
    }
}

/// The clause 4.9 target value an attribute instance's family carries, or
/// `None` when none of its values is one.
///
/// A structured Property value carries no literal of its own, and neither does
/// a geometry, so neither is a target: a path addressing a member of a
/// structured value descends into it instead.
#[must_use]
pub fn family_target<'a, I: QueriedInstance<'a>>(instance: I) -> Option<ResolvedTarget<'a>> {
    let reading = family_reading(instance.family());
    let mut terminals = instance.values().filter_map(|item| reading.terminal.read(item));
    let value = match reading.carrier {
        ValueCarrier::Nodes => {
            let first = terminals.next()?;
            match terminals.next() {
                None => first,
                Some(second) => TargetValue::Array([first, second].into_iter().chain(terminals).collect()),
            }
        }
        ValueCarrier::OrderedList => TargetValue::Array(terminals.collect()),
    };
    Some(ResolvedTarget {
        value,
        origin: reading.origin,
        reading: reading.reading,
    })
}

/// The first value of an attribute instance's family member, which is the node
/// a compound value's members hang off and the value a path naming the member
/// itself (`speed.value`) lands on.
///
/// A list family holds its items inside one list, which has neither a literal
/// nor members of its own, so a path into it addresses nothing.
#[must_use]
pub fn first_family_value<'a, I: QueriedInstance<'a>>(instance: I) -> Option<ValueItem<'a, I::Node>> {
    match family_reading(instance.family()).carrier {
        ValueCarrier::Nodes => instance.values().next(),
        ValueCarrier::OrderedList => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{family_target::family_target, target_value::TargetOrigin},
        queried_entity::expanded_fixture::{FixtureInstance, target_json},
    };
    use serde_json::{Value, json};

    /// The target value one expanded attribute instance carries, panicking when
    /// it carries none.
    fn target_of(instance: &Value) -> (Value, TargetOrigin) {
        let instance = FixtureInstance::from_expanded(instance);
        let target = family_target(&instance).expect("a target value");
        (target_json(&target.value), target.origin)
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
    /// is what keeps the ordering and pattern operators — defined over a scalar
    /// and a String target — off it.
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
    /// resolves no target value and no `==` term matches it; existence reads
    /// the attribute, not its target value.
    #[test]
    fn an_empty_array_valued_property_resolves_no_target() {
        let instance = FixtureInstance::from_expanded(&json!({"https://uri.etsi.org/ngsi-ld/hasValue": []}));
        assert!(family_target(&instance).is_none());
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
    /// latter is an array" — so every term is a target, not only the first.
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
    /// it is not a terminal target: a path descends into its member nodes
    /// instead.
    #[test]
    fn a_structured_property_value_resolves_no_terminal_target() {
        let instance = FixtureInstance::from_expanded(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.com/city": [{"@value": "Berlin"}]}]
        }));
        assert!(family_target(&instance).is_none());
    }

    /// A `GeoProperty` held as a geometry carries no literal clause 4.9 could
    /// compare, while the clause 4.7.2 string form is a String like any other.
    #[test]
    fn a_geometry_is_no_target_and_its_string_form_is_text() {
        let geometry = FixtureInstance::from_expanded(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1}, {"@value": 2}]}]
            }]
        }));
        assert!(family_target(&geometry).is_none());
        let (value, _) = target_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}]
        }));
        pretty_assertions::assert_eq!(value, json!("{\"type\":\"Point\",\"coordinates\":[1,2]}"));
    }
}
