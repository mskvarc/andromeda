//! The Context Source Properties of a registration as the `q` evaluator reads
//! them (clauses 4.9, 5.2.9 and 5.11.2.4).
//!
//! A `csf` filter targets "the name of a Context Source Property member of the
//! `CSourceRegistration`" (clause 4.9). Those members are not an expanded
//! entity: each is held under its expanded name as the registration spells it,
//! a normalized `{"type": "Property", "value": ...}` node, a bare JSON value,
//! or an array of either, every element of which is one value of the property,
//! and the value is raw JSON whose keys a compound path addresses verbatim. A
//! value object whose `@type` declares a clause 4.6.3 time value, by the core
//! context's term or by its IRI, is that time value (annex C.6), the one
//! reading a time operand compares with.

use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    time::value::{TimeValue, TimeValueForm},
};
use andromeda_ngsi_ld_query::{
    filter::{language_matching::LanguageMatching, target_language::LanguageSelector},
    queried_entity::{
        attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
        instance_metadata::InstanceMetadata,
        value_item::{LiteralValue, ValueItem},
    },
};
use andromeda_ngsi_ld_vocabulary::{compact::classes as compact_classes, keyword};
use std::{
    borrow::Cow,
    iter::{empty, once},
};

/// The members of a registration a `csf` filter reads: a JSON object keyed by
/// expanded name.
#[derive(Clone, Copy, Debug)]
pub struct ContextSourceProperties<'a>(JsonPiece<'a>);

impl<'a> ContextSourceProperties<'a> {
    /// The properties held by a JSON object; `None` for any other JSON, which
    /// holds no property a filter could name.
    #[must_use]
    pub fn of(properties: JsonPiece<'a>) -> Option<ContextSourceProperties<'a>> {
        (properties.kind() == JsonKind::Object).then_some(ContextSourceProperties(properties))
    }

    /// The properties of a JSON object its builder wrote as one.
    pub(crate) const fn of_object(object: JsonPiece<'a>) -> ContextSourceProperties<'a> {
        ContextSourceProperties(object)
    }

    /// The value of the property `name`, read in place.
    fn member(self, name: &AttributeName) -> Option<JsonPiece<'a>> {
        self.0.member(name.as_str())
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
    /// it carries none, as a bare JSON value or a structured member such as
    /// `management` does.
    fn held(self) -> JsonPiece<'a> {
        match self.0.kind() {
            JsonKind::Object => VALUE_KEYS.into_iter().find_map(|key| self.0.member(key)).unwrap_or(self.0),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Array => self.0,
        }
    }

    /// The one value the node holds as the `q` evaluator reads it: a declared
    /// time value, written as the node or as its normalized `value`, or the
    /// raw JSON it holds.
    fn item(self) -> ValueItem<'a, NoStructuredValue> {
        let held = self.held();
        declared_time(self.0)
            .or_else(|| declared_time(held))
            .map_or(ValueItem::Json(held), |time| ValueItem::Literal(LiteralValue::Time(time)))
    }

    /// The first value of the member `name` of the node.
    fn member(self, name: &str) -> Option<PropertyValue<'a>> {
        member_values(self.0.member(name)?).into_iter().next()
    }
}

/// The time value a value object declares by its `@type`, `None` for any
/// other JSON and for a literal its declared form does not admit.
fn declared_time(node: JsonPiece<'_>) -> Option<TimeValue> {
    let datatype = node.member(keyword::TYPE)?.as_str()?;
    let form = [
        (TimeValueForm::DateTime, compact_classes::DATE_TIME),
        (TimeValueForm::Date, compact_classes::DATE),
        (TimeValueForm::Time, compact_classes::TIME),
    ]
    .into_iter()
    .find_map(|(form, term)| (datatype == term || datatype == form.iri().as_str()).then_some(form))?;
    form.parse(&node.member(keyword::VALUE)?.as_str()?).ok()
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

    // The type has no value, so no node of it holds a member.
    fn member_values(self, _member: &AttributeName) -> impl Iterator<Item = ValueItem<'a, NoStructuredValue>> {
        empty()
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
        once(self.item())
    }

    /// Clause 5.2.9 Context Source Properties are Properties, never a
    /// `LanguageProperty`, so no target language selects a string of theirs.
    fn language_values(self, _wanted: LanguageSelector<'_>, _matching: LanguageMatching) -> impl Iterator<Item = ValueItem<'a, NoStructuredValue>> {
        empty()
    }

    fn sub_attribute_instances(self, name: &AttributeName) -> impl Iterator<Item = PropertyValue<'a>> {
        self.member(name.as_str()).into_iter()
    }

    fn metadata(self, member: InstanceMetadata) -> impl Iterator<Item = Cow<'a, str>> {
        self.member(member.iri().as_str()).and_then(|held| held.held().as_str()).into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::context_source_properties::ContextSourceProperties;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_query::{
        filter::{eval::evaluate, language_matching::LanguageMatching, parser::parse},
        queried_entity::linked_entities::NoLinkedEntities,
    };
    use serde_json::{Value, json};

    /// Whether the filter holds over the properties, read through the entity
    /// traits.
    fn holds(q: &str, properties: &Value) -> bool {
        let expr = parse(q).expect("the filter parses");
        let properties = JsonLiteral::admit(&properties.to_string()).expect("a JSON literal");
        ContextSourceProperties::of(properties.piece()).is_some_and(|properties| evaluate(&expr, properties, NoLinkedEntities, LanguageMatching::TagEquality))
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
        assert!(ContextSourceProperties::of(JsonLiteral::admit("[]").expect("a JSON literal").piece()).is_none());
    }
}
