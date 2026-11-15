//! The Context Source Properties of a registration as the `q` evaluator reads
//! them (clauses 4.9, 5.2.9 and 5.11.2.4).
//!
//! A `csf` filter targets "the name of a Context Source Property member of the
//! `CSourceRegistration`" (clause 4.9). Those members are not an expanded
//! entity: each is held under its expanded name as the registration spells it,
//! and each is one Property, since the instances of clause 4.5.5 are an
//! Entity's. A JSON object holding a `value` member is the Property's node
//! (clause 4.5.2.3: normalized when it carries `type`, and a Property inferred
//! from `value` when it does not): its value is that member, and its other
//! members are its sub-properties and instance members. Any other JSON (a
//! scalar, an array, an object holding no `value` such as `management` or a
//! `GeoJSON` geometry) is the value itself, an array one array value clause 4.9
//! compares by membership. The value is raw JSON whose keys a compound path
//! addresses verbatim. A value object whose `@type` declares a clause 4.6.3
//! time value, by the core context's term or by its IRI, is that time value
//! (annex C.6), the one reading a time operand compares with. The SQL
//! rendering of a `csf` reads a stored document by these same rules.

use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    time::value::{TimeValue, TimeValueForm},
};
use andromeda_ngsi_ld_query::filter::{instance_metadata::InstanceMetadata, language_matching::LanguageMatching, target_language::LanguageSelector};
use andromeda_ngsi_ld_query_evaluation::queried_entity::{
    attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
    value_item::{LiteralValue, ValueItem},
};
use andromeda_ngsi_ld_vocabulary::{
    compact::{classes as compact_classes, properties as compact_properties},
    keyword,
};
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

/// A Context Source Property as written: its node, or a bare JSON value.
#[derive(Clone, Copy, Debug)]
pub struct PropertyValue<'a>(JsonPiece<'a>);

impl<'a> PropertyValue<'a> {
    /// The value the property holds: the `value` member of its node, or the
    /// JSON itself when it holds none.
    fn held(self) -> JsonPiece<'a> {
        self.0.member(compact_properties::VALUE).unwrap_or(self.0)
    }

    /// The one value the property holds as the `q` evaluator reads it: a
    /// declared time value or the raw JSON it holds.
    fn item(self) -> ValueItem<'a, NoStructuredValue> {
        let held = self.held();
        declared_time(held).map_or(ValueItem::Json(held), |time| ValueItem::Literal(LiteralValue::Time(time)))
    }

    /// The member `name` of the property's node: a sub-property or an instance
    /// member.
    fn member(self, name: &str) -> Option<PropertyValue<'a>> {
        self.0.member(name).map(PropertyValue)
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
        self.member(attribute).map(PropertyValue).into_iter()
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
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};
    use andromeda_ngsi_ld_query_evaluation::{q::filter_run::filter_holds, queried_entity::linked_entities::NoLinkedEntities};
    use serde_json::{Value, json};

    /// Whether the filter holds over the properties, read through the entity
    /// traits.
    fn holds(q: &str, properties: &Value) -> bool {
        let reading = FilterReading::of(&parse(q).expect("the filter parses"), LanguageMatching::TagEquality);
        let properties = JsonLiteral::admit(&properties.to_string()).expect("a JSON literal");
        ContextSourceProperties::of(properties.piece()).is_some_and(|properties| filter_holds(&reading, properties, NoLinkedEntities))
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

    /// Clause 4.9 compares an array value by membership: an equality holds when
    /// the operand is included in it, an inequality when it is not, and an
    /// order or a pattern holds for no array.
    #[test]
    fn a_bare_array_is_one_array_value() {
        let properties = json!({"https://example.com/sourceGroup": ["alpha", "beta"], "https://example.com/rank": [1, 5]});
        assert!(holds("\"https://example.com/sourceGroup\"==\"beta\"", &properties));
        assert!(!holds("\"https://example.com/sourceGroup\"==\"gamma\"", &properties));
        assert!(!holds("\"https://example.com/sourceGroup\"!=\"alpha\"", &properties));
        assert!(holds("\"https://example.com/sourceGroup\"!=\"gamma\"", &properties));
        assert!(!holds("\"https://example.com/sourceGroup\"~=\"^al\"", &properties));
        assert!(holds("\"https://example.com/rank\"==5", &properties));
        assert!(!holds("\"https://example.com/rank\">2", &properties));
        assert!(!holds("\"https://example.com/rank\"==2..6", &properties));
    }

    /// Clause 4.9 reads "a single element" of an array value, so an element
    /// that is an array is no element of its own value.
    #[test]
    fn an_array_nested_in_an_array_value_is_one_element() {
        let properties = json!({"https://example.com/nested": [1, [2]]});
        assert!(holds("\"https://example.com/nested\"==1", &properties));
        assert!(!holds("\"https://example.com/nested\"==2", &properties));
        assert!(holds("\"https://example.com/nested\"!=2", &properties));
    }

    /// Clause 4.5.2.3 infers a Property from `value`, so a concise node is read
    /// by its value, and an object holding no `value` is the value itself
    /// whatever `type` it carries.
    #[test]
    fn an_object_holding_value_is_the_node_of_the_property() {
        let properties = json!({
            "https://example.com/concise": {"value": "x", "https://example.com/since": 2020},
            "https://example.com/typed": {"type": "Property"}
        });
        assert!(holds("\"https://example.com/concise\"==\"x\"", &properties));
        assert!(holds("\"https://example.com/concise\".\"https://example.com/since\"==2020", &properties));
        assert!(holds("\"https://example.com/typed\"", &properties));
        assert!(!holds("\"https://example.com/typed\"==\"Property\"", &properties));
        assert!(holds("\"https://example.com/typed\"!=\"x\"", &properties));
        assert!(holds("\"https://example.com/typed\"[type]==\"Property\"", &properties));
    }

    /// A Context Source Property has no clause 4.5.5 instances, so an array of
    /// nodes is one array value holding objects, and an empty array is a value
    /// that includes no operand.
    #[test]
    fn an_array_of_nodes_is_one_value_and_an_empty_array_is_present() {
        let properties = json!({
            "https://example.com/nodes": [{"type": "Property", "value": "x"}],
            "https://example.com/empty": []
        });
        assert!(holds("\"https://example.com/nodes\"", &properties));
        assert!(!holds("\"https://example.com/nodes\"==\"x\"", &properties));
        assert!(holds("\"https://example.com/nodes\"!=\"x\"", &properties));
        assert!(holds("\"https://example.com/empty\"", &properties));
        assert!(holds("\"https://example.com/empty\"!=\"x\"", &properties));
        assert!(!holds("\"https://example.com/empty\"==\"x\"", &properties));
    }

    /// A JSON-LD value object holds no `value` member, so it is a structured
    /// value, and an expanded array is one array value holding it.
    #[test]
    fn an_expanded_value_is_read_as_the_json_it_is() {
        let properties = json!({
            "https://example.com/object": {"@value": "wide"},
            "https://example.com/expanded": [{"@value": "wide", "https://example.com/since": [{"@value": 2020}]}]
        });
        assert!(!holds("\"https://example.com/object\"==\"wide\"", &properties));
        assert!(holds("\"https://example.com/object\"!=\"wide\"", &properties));
        assert!(!holds("\"https://example.com/expanded\"==\"wide\"", &properties));
        assert!(!holds("\"https://example.com/expanded\".\"https://example.com/since\"==2020", &properties));
    }

    /// An instance member holding an instant (clause 4.8) is the `DateTime`
    /// its text spells, whether the node holds it bare or as a concise node,
    /// and a member that is no string is no instance member.
    #[test]
    fn an_instance_member_is_read_as_the_text_it_holds() {
        let properties = json!({"https://example.com/seen": {
            "value": 1,
            "https://uri.etsi.org/ngsi-ld/observedAt": "2031-01-01T00:00:00Z",
            "https://uri.etsi.org/ngsi-ld/unitCode": {"@value": "CEL"},
            "https://uri.etsi.org/ngsi-ld/datasetId": {"value": "urn:x:d"}
        }});
        let observed = "\"https://example.com/seen\".\"https://uri.etsi.org/ngsi-ld/observedAt\"";
        assert!(holds(&format!("{observed}>2030-01-01T00:00:00Z"), &properties));
        assert!(!holds(&format!("{observed}==\"2031-01-01T00:00:00Z\""), &properties));
        let unit = "\"https://example.com/seen\".\"https://uri.etsi.org/ngsi-ld/unitCode\"";
        assert!(!holds(unit, &properties));
        assert!(!holds(&format!("{unit}==\"CEL\""), &properties));
        assert!(holds(
            "\"https://example.com/seen\".\"https://uri.etsi.org/ngsi-ld/datasetId\"==\"urn:x:d\"",
            &properties
        ));
    }

    /// A compound path reads the raw JSON value by the key the registrant
    /// wrote (clause 4.9 trailing path).
    #[test]
    fn a_compound_value_is_read_by_its_verbatim_key() {
        let properties = json!({"https://example.com/coverage": {"type": "Property", "value": {"region": "north"}}});
        assert!(holds("\"https://example.com/coverage\"[region]==\"north\"", &properties));
        assert!(!holds("\"https://example.com/coverage\"[region]==\"south\"", &properties));
    }

    /// A sub-property is a member of its property's node beside the node's
    /// value, and a compound member is a key of the value, so an existence term
    /// reads each where a comparison reads it.
    #[test]
    fn existence_reads_a_sub_property_beside_the_value_and_a_member_inside_it() {
        let nested = json!({"https://example.com/a": {
            "type": "Property",
            "value": {"m": 1},
            "b": {"type": "Property", "value": 2}
        }});
        let flat = json!({"https://example.com/a": {"type": "Property", "value": 1}});
        assert!(holds("\"https://example.com/a\".b", &nested));
        assert!(holds("\"https://example.com/a\".b==2", &nested));
        assert!(!holds("\"https://example.com/a\".b", &flat));
        assert!(holds("\"https://example.com/a\"[m]", &nested));
        assert!(!holds("\"https://example.com/a\"[m]", &flat));
        assert!(holds("\"https://example.com/a\"", &flat));
    }

    #[test]
    fn json_that_is_no_object_holds_no_property() {
        assert!(!holds("\"https://example.com/sourceGroup\"", &json!("alpha")));
        assert!(ContextSourceProperties::of(JsonLiteral::admit("[]").expect("a JSON literal").piece()).is_none());
    }
}
