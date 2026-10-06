//! The Context Source Properties of a registration as the `q` evaluator reads
//! them (clauses 4.9, 5.2.9 and 5.11.2.4).
//!
//! A `csf` filter targets "the name of a Context Source Property member of the
//! `CSourceRegistration`" (clause 4.9). Those members are not an expanded
//! entity: each is held under its expanded name as the registrant wrote it --
//! a normalized `{"type": "Property", "value": ...}` node, a bare JSON value,
//! or an array of either, every element of which is one value of the property
//! -- and the value is raw JSON whose keys a compound path addresses verbatim.

use andromeda_json_literal::{
    JsonLiteral,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::queried_entity::{
    attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
    instance_metadata::InstanceMetadata,
    value_item::ValueItem,
};
use indexmap::IndexMap;
use std::{borrow::Cow, iter::empty};

/// The members of a registration a `csf` filter reads, keyed by expanded name.
#[derive(Clone, Copy, Debug)]
pub struct ContextSourceProperties<'a>(PropertyMembers<'a>);

/// Where the members are held: the members of a registration, or the JSON
/// object a document of them was written as.
#[derive(Clone, Copy, Debug)]
enum PropertyMembers<'a> {
    /// The Context Source Properties of a registration.
    Held(&'a IndexMap<AttributeName, JsonLiteral>),
    /// A JSON object whose members are the properties.
    Written(JsonPiece<'a>),
}

impl<'a> ContextSourceProperties<'a> {
    /// The properties held by a JSON object; `None` for any other JSON, which
    /// holds no property a filter could name.
    #[must_use]
    pub fn of(properties: JsonPiece<'a>) -> Option<ContextSourceProperties<'a>> {
        (properties.kind() == JsonKind::Object).then_some(ContextSourceProperties(PropertyMembers::Written(properties)))
    }

    /// The Context Source Properties a registration holds.
    #[must_use]
    pub const fn of_registration(properties: &'a IndexMap<AttributeName, JsonLiteral>) -> ContextSourceProperties<'a> {
        ContextSourceProperties(PropertyMembers::Held(properties))
    }

    /// The value of the property `name`, read in place.
    fn member(self, name: &AttributeName) -> Option<JsonPiece<'a>> {
        match self.0 {
            PropertyMembers::Held(properties) => properties.get(name).map(JsonLiteral::piece),
            PropertyMembers::Written(properties) => properties.member(name.as_str()),
        }
    }
}

/// One value of a Context Source Property: a normalized node or a bare JSON
/// value.
#[derive(Clone, Copy, Debug)]
pub struct PropertyValue<'a>(JsonPiece<'a>);

/// The keys a property node carries its value under, in the order they are
/// read: the expanded JSON-LD keywords and the compact `value` of a normalized
/// property.
const VALUE_KEYS: [&str; 3] = ["@value", "@id", "value"];

impl<'a> PropertyValue<'a> {
    /// The value the node holds: under its value key, or the node itself when
    /// it is a bare JSON value; nothing for an object without a value.
    fn held(self) -> Option<JsonPiece<'a>> {
        match self.0.kind() {
            JsonKind::Object => VALUE_KEYS.into_iter().find_map(|key| self.0.member(key)),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Array => Some(self.0),
        }
    }

    /// The first value of the member `name` of the node.
    fn member(self, name: &str) -> Option<PropertyValue<'a>> {
        member_values(self.0.member(name)?).into_iter().next()
    }
}

/// Each value a member holds: every element of an array, or the member itself.
fn member_values(member: JsonPiece<'_>) -> Vec<PropertyValue<'_>> {
    match member.elements() {
        Some(elements) => elements.into_iter().map(PropertyValue).collect(),
        None => vec![PropertyValue(member)],
    }
}

/// A registration property holds raw JSON and never an expanded structured
/// value, so it has no node a compound path could descend through by IRI: the
/// path reads the raw JSON by its verbatim keys instead.
#[derive(Clone, Copy, Debug)]
pub enum NoStructuredValue {}

impl<'a> QueriedValueNode<'a> for NoStructuredValue {
    fn id(self) -> Option<&'a str> {
        match self {}
    }

    fn first_member_value(self, _member: &AttributeName) -> Option<ValueItem<'a, NoStructuredValue>> {
        match self {}
    }
}

impl<'a> QueriedEntity<'a> for ContextSourceProperties<'a> {
    type Instance = PropertyValue<'a>;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = PropertyValue<'a>> {
        self.member(attribute).map(member_values).into_iter().flatten()
    }

    /// A registration names no entity type of its own.
    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        empty()
    }
}

impl<'a> QueriedInstance<'a> for PropertyValue<'a> {
    type Node = NoStructuredValue;

    /// Clause 5.2.9 Context Source Properties are Properties.
    fn family(self) -> AttributeType {
        AttributeType::Property
    }

    fn values(self) -> impl Iterator<Item = ValueItem<'a, NoStructuredValue>> {
        self.held().map(ValueItem::Json).into_iter()
    }

    /// Clause 5.2.9 Context Source Properties are Properties, never a
    /// `LanguageProperty`, so no target language selects a string of theirs.
    fn language_values(self, _language: &str) -> impl Iterator<Item = ValueItem<'a, NoStructuredValue>> {
        empty()
    }

    fn sub_attribute(self, name: &AttributeName) -> Option<PropertyValue<'a>> {
        self.member(name.as_str())
    }

    fn metadata(self, member: InstanceMetadata) -> Option<Cow<'a, str>> {
        self.member(member.iri().as_str())?.held()?.as_str()
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::context_source_properties::ContextSourceProperties;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        filter::{eval::evaluate, parser::parse},
        queried_entity::linked_entities::NoLinkedEntities,
    };
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    /// Whether the filter holds over the properties, read through the entity
    /// traits.
    fn holds(q: &str, properties: &Value) -> bool {
        let expr = parse(q).expect("the filter parses");
        let properties = JsonLiteral::admit(&properties.to_string()).expect("a JSON literal");
        ContextSourceProperties::of(properties.piece()).is_some_and(|properties| evaluate(&expr, properties, NoLinkedEntities))
    }

    #[test]
    fn a_normalized_property_is_read_by_its_value() {
        let properties = json!({"https://example.com/sourceGroup": {"type": "Property", "value": "alpha"}});
        assert!(holds("\"https://example.com/sourceGroup\"==\"alpha\"", &properties));
        assert!(!holds("\"https://example.com/sourceGroup\"==\"beta\"", &properties));
    }

    #[test]
    fn a_scalar_property_is_its_own_value() {
        let properties = json!({"https://example.com/sourceGroup": "alpha", "https://example.com/tier": 3});
        assert!(holds("\"https://example.com/sourceGroup\"==\"alpha\"", &properties));
        assert!(holds("\"https://example.com/tier\">2", &properties));
        assert!(holds("\"https://example.com/tier\"", &properties));
        assert!(!holds("\"https://example.com/other\"", &properties));
    }

    /// An array is matched by any of its elements, each read as one value.
    #[test]
    fn an_array_property_matches_any_element() {
        let properties = json!({"https://example.com/sourceGroup": ["alpha", "beta"]});
        assert!(holds("\"https://example.com/sourceGroup\"==\"beta\"", &properties));
        assert!(!holds("\"https://example.com/sourceGroup\"==\"gamma\"", &properties));
    }

    /// A compound path reads the raw JSON value by the key the registrant
    /// wrote (clause 4.9 trailing path).
    #[test]
    fn a_compound_value_is_read_by_its_verbatim_key() {
        let properties = json!({"https://example.com/coverage": {"type": "Property", "value": {"region": "north"}}});
        assert!(holds("\"https://example.com/coverage\"[region]==\"north\"", &properties));
        assert!(!holds("\"https://example.com/coverage\"[region]==\"south\"", &properties));
    }

    /// A sub-property is read through `.`, as clause 4.9 reads a
    /// sub-attribute.
    #[test]
    fn a_sub_property_is_read_through_the_dot_axis() {
        let properties = json!({"https://example.com/coverage": [{
            "@value": "wide",
            "https://example.com/since": [{"@value": 2020}]
        }]});
        assert!(holds("\"https://example.com/coverage\".\"https://example.com/since\"==2020", &properties));
    }

    /// The members a registration holds are read as the object they are
    /// written as is.
    #[test]
    fn the_properties_a_registration_holds_are_read_as_their_written_object_is() {
        let held: IndexMap<AttributeName, JsonLiteral> =
            serde_json::from_str(r#"{"https://example.com/sourceGroup": {"type": "Property", "value": "alpha"}, "https://example.com/tier": 3}"#)
                .expect("the members of a registration");
        let properties = ContextSourceProperties::of_registration(&held);
        for q in ["\"https://example.com/sourceGroup\"==\"alpha\"", "\"https://example.com/tier\">2"] {
            assert!(evaluate(&parse(q).expect("the filter parses"), properties, NoLinkedEntities), "{q}");
        }
        assert!(!evaluate(
            &parse("\"https://example.com/other\"").expect("the filter parses"),
            properties,
            NoLinkedEntities
        ));
    }

    #[test]
    fn json_that_is_no_object_holds_no_property() {
        assert!(!holds("\"https://example.com/sourceGroup\"", &json!("alpha")));
        assert!(ContextSourceProperties::of(JsonLiteral::admit("[]").expect("a JSON literal").piece()).is_none());
    }
}
