//! A test entity model read off expanded JSON-LD fixtures, implementing the
//! entity traits the evaluators read.
//!
//! The evaluator tests are written against the expanded documents a stored
//! entity and a notification payload carry. Decoding a fixture once into this
//! owned model, shaped the way a typed entity is (one family per instance,
//! instance members held as data, sub-attributes keyed by name), lets the same
//! fixtures drive the trait-based evaluators.

use crate::{
    filter::{language_matching::LanguageMatching, target_language::LanguageSelector, target_value::TargetValue},
    queried_entity::{
        attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
        fixture_literal::FixtureLiteral,
        instance_metadata::InstanceMetadata,
        linked_entities::LinkedEntities,
        value_item::ValueItem,
    },
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_vocabulary::expanded::properties::OBJECT;
use langtag::LangTag;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::borrow::Cow;
use strum::IntoEnumIterator;

/// The families in the order an expanded instance is searched for a value
/// member.
const FAMILY_SEARCH_ORDER: [AttributeType; 7] = [
    AttributeType::Property,
    AttributeType::Relationship,
    AttributeType::VocabProperty,
    AttributeType::LanguageProperty,
    AttributeType::JsonProperty,
    AttributeType::ListProperty,
    AttributeType::ListRelationship,
];

/// An entity decoded from an expanded fixture.
#[derive(Debug)]
pub(crate) struct FixtureEntity {
    /// The entity's `@id`.
    id: Option<String>,
    /// The entity's expanded types.
    types: Vec<EntityTypeName>,
    /// Every attribute with its instances, in fixture order.
    attributes: Vec<(AttributeName, Vec<FixtureInstance>)>,
}

/// An attribute instance decoded from an expanded fixture.
#[derive(Debug)]
pub(crate) struct FixtureInstance {
    /// The family, told by the value member the instance carries.
    family: AttributeType,
    /// The values of the family's value member.
    values: Vec<FixtureItem>,
    /// The instance members held as data, each with every node it holds.
    metadata: Vec<(InstanceMetadata, String)>,
    /// The sub-attributes with their instances.
    sub_attributes: Vec<(AttributeName, Vec<FixtureInstance>)>,
}

/// One value decoded from an expanded fixture.
#[derive(Debug)]
pub(crate) enum FixtureItem {
    /// A literal.
    Literal(FixtureLiteral),
    /// Raw JSON.
    Json(JsonLiteral),
    /// An IRI reference.
    Reference(String),
    /// A node with IRI-keyed members.
    Node(FixtureNode),
    /// A nested list.
    Opaque,
}

/// A node of a structured value decoded from an expanded fixture.
#[derive(Debug)]
pub(crate) struct FixtureNode {
    /// The node's `@id`.
    id: Option<String>,
    /// The node's members with their values.
    members: Vec<(AttributeName, Vec<FixtureItem>)>,
}

impl FixtureEntity {
    /// Decode an expanded entity object or its one-element array document.
    pub(crate) fn from_expanded(document: &Value) -> FixtureEntity {
        let node = match document {
            Value::Array(nodes) => nodes.first().expect("a one-element expanded document"),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => document,
        };
        let object = node.as_object().expect("an expanded entity object");
        let types = object.get("@type").and_then(Value::as_array).map_or_default(|types| {
            types
                .iter()
                .map(|declared| declared.as_str().expect("a type IRI").parse().expect("a type name"))
                .collect()
        });
        let attributes = object
            .iter()
            .filter(|(key, _)| !key.starts_with('@'))
            .map(|(key, instances)| (attribute_name(key), instances_of(instances)))
            .collect();
        let id = object.get("@id").and_then(Value::as_str).map(str::to_owned);
        FixtureEntity { id, types, attributes }
    }
}

/// The fixture entities a hop may cross into, found by their `@id`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FixtureLinked<'a>(pub(crate) &'a [FixtureEntity]);

impl<'a> LinkedEntities<'a> for FixtureLinked<'a> {
    type Entity = &'a FixtureEntity;

    fn linked(self, object: &str) -> Option<&'a FixtureEntity> {
        self.0.iter().find(|entity| entity.id.as_deref() == Some(object))
    }
}

impl FixtureInstance {
    /// Decode one expanded attribute instance.
    pub(crate) fn from_expanded(instance: &Value) -> FixtureInstance {
        let object = instance.as_object().expect("an expanded attribute instance");
        let family = family_of(object);
        let value_key = family.value_key();
        let values = object
            .get(value_key.as_str())
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |nodes| match family {
                AttributeType::ListProperty | AttributeType::ListRelationship => nodes
                    .first()
                    .and_then(|list| list.get("@list"))
                    .and_then(Value::as_array)
                    .map_or_else(Vec::new, |items| items.iter().map(item_of).collect()),
                AttributeType::Property
                | AttributeType::Relationship
                | AttributeType::GeoProperty
                | AttributeType::VocabProperty
                | AttributeType::LanguageProperty
                | AttributeType::JsonProperty => nodes.iter().map(item_of).collect(),
            });
        let metadata = InstanceMetadata::iter()
            .flat_map(|member| {
                object
                    .get(member.iri().as_str())
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(move |node| Some((member, node.get("@value").or_else(|| node.get("@id"))?.as_str()?.to_owned())))
            })
            .collect();
        let sub_attributes = object
            .iter()
            .filter(|(key, _)| {
                !key.starts_with('@') && key.as_str() != value_key.as_str() && InstanceMetadata::iter().all(|member| member.iri().as_str() != key.as_str())
            })
            .map(|(key, instances)| (attribute_name(key), instances_of(instances)))
            .collect();
        FixtureInstance {
            family,
            values,
            metadata,
            sub_attributes,
        }
    }
}

/// An attribute name read off a fixture key.
fn attribute_name(key: &str) -> AttributeName {
    key.parse().expect("an attribute name")
}

/// The instances an expanded attribute member holds.
fn instances_of(member: &Value) -> Vec<FixtureInstance> {
    member
        .as_array()
        .expect("an expanded attribute is an array of instances")
        .iter()
        .map(FixtureInstance::from_expanded)
        .collect()
}

/// The family an expanded instance belongs to: a `GeoProperty` by its declared
/// type, every other family by the value member it carries.
fn family_of(object: &Map<String, Value>) -> AttributeType {
    let declared_geo = object
        .get("@type")
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|declared| declared.as_str() == Some(AttributeType::GeoProperty.iri_str())));
    if declared_geo {
        return AttributeType::GeoProperty;
    }
    FAMILY_SEARCH_ORDER
        .into_iter()
        .find(|family| object.contains_key(family.value_key().as_str()))
        .unwrap_or(AttributeType::Property)
}

/// Decode one expanded value node.
fn item_of(node: &Value) -> FixtureItem {
    let Some(object) = node.as_object() else {
        return FixtureItem::Opaque;
    };
    if let Some(wrapped) = object.get(OBJECT.as_str()).and_then(Value::as_array).and_then(|wrapped| wrapped.first()) {
        return item_of(wrapped);
    }
    if object.contains_key("@list") {
        return FixtureItem::Opaque;
    }
    if let Some(value) = object.get("@value") {
        let is_json = object.get("@type").and_then(Value::as_str) == Some("@json");
        if let (Value::String(text), Some(language)) = (value, object.get("@language").and_then(Value::as_str)) {
            return FixtureItem::Literal(FixtureLiteral::Tagged {
                text: text.to_owned(),
                language: language.to_owned(),
            });
        }
        return match value {
            Value::Number(number) if !is_json => FixtureItem::Literal(FixtureLiteral::Number(JsonNumber::try_from(number).expect("an exact number"))),
            Value::String(text) if !is_json => FixtureItem::Literal(FixtureLiteral::declared(object.get("@type").and_then(Value::as_str), text)),
            Value::Bool(boolean) if !is_json => FixtureItem::Literal(FixtureLiteral::Boolean(*boolean)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
                FixtureItem::Json(JsonLiteral::deserialize(value).expect("a JSON literal"))
            }
        };
    }
    let id = object.get("@id").and_then(Value::as_str).map(str::to_owned);
    let members: Vec<(AttributeName, Vec<FixtureItem>)> = object
        .iter()
        .filter(|(key, _)| !key.starts_with('@'))
        .map(|(key, values)| {
            (
                attribute_name(key),
                values.as_array().map_or_else(Vec::new, |values| values.iter().map(item_of).collect()),
            )
        })
        .collect();
    match id {
        Some(id) if members.is_empty() => FixtureItem::Reference(id),
        Some(id) => FixtureItem::Node(FixtureNode { id: Some(id), members }),
        None => FixtureItem::Node(FixtureNode { id: None, members }),
    }
}

impl FixtureItem {
    /// The item as the evaluator reads it.
    fn view(&self) -> ValueItem<'_, &FixtureNode> {
        match self {
            FixtureItem::Literal(literal) => ValueItem::Literal(literal.view()),
            FixtureItem::Json(json) => ValueItem::Json(json.piece()),
            FixtureItem::Reference(iri) => ValueItem::Reference(iri),
            FixtureItem::Node(node) => ValueItem::Node(node),
            FixtureItem::Opaque => ValueItem::Opaque,
        }
    }
}

impl<'a> QueriedEntity<'a> for &'a FixtureEntity {
    type Instance = &'a FixtureInstance;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a FixtureInstance> {
        self.attributes
            .iter()
            .filter(move |(name, _)| name == attribute)
            .flat_map(|(_, instances)| instances)
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        self.types.iter()
    }
}

impl<'a> QueriedInstance<'a> for &'a FixtureInstance {
    type Node = &'a FixtureNode;

    fn family(self) -> AttributeType {
        self.family
    }

    fn values(self) -> impl Iterator<Item = ValueItem<'a, &'a FixtureNode>> {
        self.values.iter().map(FixtureItem::view)
    }

    fn language_values(self, wanted: LanguageSelector<'_>, matching: LanguageMatching) -> impl Iterator<Item = ValueItem<'a, &'a FixtureNode>> {
        let family = self.family;
        self.values
            .iter()
            .filter(move |item| {
                family == AttributeType::LanguageProperty
                    && matches!(item, FixtureItem::Literal(FixtureLiteral::Tagged { language: tag, .. })
                        if LangTag::new(tag.as_str()).is_ok_and(|tag| wanted.admits(tag, matching)))
            })
            .map(FixtureItem::view)
    }

    fn sub_attribute_instances(self, name: &AttributeName) -> impl Iterator<Item = &'a FixtureInstance> {
        self.sub_attributes
            .iter()
            .find(|(sub_attribute, _)| sub_attribute == name)
            .into_iter()
            .flat_map(|(_, instances)| instances)
    }

    fn metadata(self, member: InstanceMetadata) -> impl Iterator<Item = Cow<'a, str>> {
        self.metadata
            .iter()
            .filter(move |(held, _)| *held == member)
            .map(|(_, text)| Cow::Borrowed(text.as_str()))
    }
}

impl<'a> QueriedValueNode<'a> for &'a FixtureNode {
    fn id(self) -> Option<&'a str> {
        self.id.as_deref()
    }

    fn member_values(self, member: &AttributeName) -> impl Iterator<Item = ValueItem<'a, &'a FixtureNode>> {
        self.members
            .iter()
            .find(|(name, _)| name == member)
            .into_iter()
            .flat_map(|(_, values)| values.iter().map(FixtureItem::view))
    }
}

/// A target value as the JSON it compares like, so expectations read as the
/// JSON a client wrote.
pub(crate) fn target_json(value: &TargetValue<'_>) -> Value {
    match value {
        TargetValue::Number(number) => Value::Number(number.to_serde_number()),
        TargetValue::Text(text) => Value::String(text.to_string()),
        TargetValue::Boolean(boolean) => Value::Bool(*boolean),
        TargetValue::Json(json) => serde_json::from_str(json.text()).expect("the JSON a literal spells"),
        TargetValue::Array(elements) => Value::Array(elements.iter().map(target_json).collect()),
        TargetValue::Time(time) => Value::String(time.to_string()),
        TargetValue::NonFiniteNumber(value) => Value::String(value.to_string()),
    }
}
