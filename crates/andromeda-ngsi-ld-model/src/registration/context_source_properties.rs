//! The Context Source Properties of a registration as the `q` evaluator reads
//! them (clauses 4.9, 5.2.9 and 5.11.2.4).
//!
//! A `csf` filter targets "the name of a Context Source Property member of the
//! `CSourceRegistration`" (clause 4.9). Those members are not an expanded
//! entity: each is held under its expanded name as the registrant wrote it --
//! a normalized `{"type": "Property", "value": ...}` node, a bare JSON value,
//! or an array of either, every element of which is one value of the property
//! -- and the value is raw JSON whose keys a compound path addresses verbatim.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::queried_entity::{
    attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
    instance_metadata::InstanceMetadata,
    value_item::ValueItem,
};
use serde_json::{Map, Value};
use std::{borrow::Cow, iter::empty, slice::from_ref};

/// The members of a registration a `csf` filter reads, keyed by expanded name.
#[derive(Clone, Copy, Debug)]
pub struct ContextSourceProperties<'a>(&'a Map<String, Value>);

impl<'a> ContextSourceProperties<'a> {
    /// The properties held by a JSON object; `None` for any other JSON, which
    /// holds no property a filter could name.
    #[must_use]
    pub const fn of(properties: &'a Value) -> Option<ContextSourceProperties<'a>> {
        match properties {
            Value::Object(members) => Some(ContextSourceProperties(members)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
        }
    }

    /// The properties held by the members of one JSON object.
    #[must_use]
    pub const fn new(members: &'a Map<String, Value>) -> ContextSourceProperties<'a> {
        ContextSourceProperties(members)
    }
}

/// One value of a Context Source Property: a normalized node or a bare JSON
/// value.
#[derive(Clone, Copy, Debug)]
pub struct PropertyValue<'a>(&'a Value);

/// The keys a property node carries its value under, in the order they are
/// read: the expanded JSON-LD keywords and the compact `value` of a normalized
/// property.
const VALUE_KEYS: [&str; 3] = ["@value", "@id", "value"];

impl<'a> PropertyValue<'a> {
    /// The value the node holds: under its value key, or the node itself when
    /// it is a bare JSON scalar; nothing for an object without a value.
    fn held(self) -> Option<&'a Value> {
        match self.0 {
            Value::Object(node) => VALUE_KEYS.into_iter().find_map(|key| node.get(key)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Some(self.0),
        }
    }

    /// The first value of the member `name` of the node.
    fn member(self, name: &str) -> Option<PropertyValue<'a>> {
        let member = self.0.as_object()?.get(name)?;
        match member {
            Value::Array(values) => values.first().map(PropertyValue),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => Some(PropertyValue(member)),
        }
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
        let values = match self.0.get(attribute.as_str()) {
            Some(Value::Array(values)) => values.as_slice(),
            Some(value) => from_ref(value),
            None => &[],
        };
        values.iter().map(PropertyValue)
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
        self.member(member.iri().as_str())?.held()?.as_str().map(Cow::Borrowed)
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::context_source_properties::ContextSourceProperties;
    use andromeda_ngsi_ld_query::{
        filter::{eval::evaluate, parser::parse},
        queried_entity::linked_entities::NoLinkedEntities,
    };
    use serde_json::{Value, json};

    /// Whether the filter holds over the properties, read through the entity
    /// traits.
    fn holds(q: &str, properties: &Value) -> bool {
        let expr = parse(q).expect("the filter parses");
        ContextSourceProperties::of(properties).is_some_and(|properties| evaluate(&expr, properties, NoLinkedEntities))
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

    #[test]
    fn json_that_is_no_object_holds_no_property() {
        assert!(!holds("\"https://example.com/sourceGroup\"", &json!("alpha")));
        assert!(ContextSourceProperties::of(&json!([])).is_none());
    }
}
