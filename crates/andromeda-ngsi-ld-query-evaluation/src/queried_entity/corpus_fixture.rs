//! The corpus Entities as the expanded JSON-LD documents a store holds, read
//! through [`FixtureEntity`] by the evaluator tests.

use crate::queried_entity::expanded_fixture::FixtureEntity;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_query::filter::instance_metadata::InstanceMetadata;
use andromeda_q_corpus::{
    attribute_description::{CorpusAttribute, CorpusContent, CorpusDatatype, CorpusInstance, CorpusTarget, CorpusValue},
    corpus_entity::{CORPUS_TYPE, CorpusEntity},
    corpus_vocabulary::CorpusVocabulary,
};
use serde_json::{Map, Value, json};
use strum::IntoEnumIterator;

/// The identifier of `entity` as a URN of the corpus type.
pub(crate) fn corpus_id(entity: CorpusEntity) -> String {
    format!("urn:ngsi-ld:{CORPUS_TYPE}:{entity}")
}

/// Every corpus Entity, decoded in the order [`CorpusEntity::iter`] lists
/// them, so a hop finds the Entity a Relationship names by its identifier.
pub(crate) fn corpus_entities() -> Vec<FixtureEntity> {
    CorpusEntity::iter()
        .map(|entity| FixtureEntity::from_expanded(&expanded_document(entity)))
        .collect()
}

/// The expanded document of `entity`.
fn expanded_document(entity: CorpusEntity) -> Value {
    let mut document = Map::new();
    document.insert("@id".to_owned(), json!(corpus_id(entity)));
    document.insert("@type".to_owned(), json!([CorpusVocabulary::vocabulary_iri(CORPUS_TYPE)]));
    insert_attributes(&mut document, entity.attributes());
    Value::Array(vec![Value::Object(document)])
}

/// Adds each of `attributes` to `members` under its expanded name.
fn insert_attributes(members: &mut Map<String, Value>, attributes: &[CorpusAttribute]) {
    for attribute in attributes {
        let instances = attribute.instances.iter().map(expanded_instance).collect();
        members.insert(CorpusVocabulary::attribute_iri(attribute.term), Value::Array(instances));
    }
}

/// One instance as its expanded members: the family's value member, then the
/// members the instance holds as data, then its sub-Attributes.
fn expanded_instance(instance: &CorpusInstance) -> Value {
    let mut members = Map::new();
    let (family, value_nodes) = content_nodes(&instance.content);
    if family == AttributeType::GeoProperty {
        members.insert("@type".to_owned(), json!([AttributeType::GeoProperty.iri_str()]));
    }
    members.insert(family.value_key_str().to_owned(), Value::Array(value_nodes));
    let metadata = [
        (InstanceMetadata::DatasetId, instance.dataset_id.map(|id| json!({"@id": id}))),
        (
            InstanceMetadata::ObservedAt,
            instance
                .observed_at
                .map(|instant| json!({"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": instant})),
        ),
        (InstanceMetadata::UnitCode, instance.unit_code.map(|code| json!({"@value": code}))),
    ];
    for (member, node) in metadata {
        if let Some(node) = node {
            members.insert(member.iri().as_str().to_owned(), json!([node]));
        }
    }
    insert_attributes(&mut members, instance.sub_attributes);
    Value::Object(members)
}

/// The family of `content` and the nodes its value member holds.
fn content_nodes(content: &CorpusContent) -> (AttributeType, Vec<Value>) {
    match content {
        CorpusContent::Property(values) => (AttributeType::Property, values.iter().map(value_node).collect()),
        CorpusContent::Relationship(targets) => (AttributeType::Relationship, targets.iter().map(target_node).collect()),
        CorpusContent::ListRelationship(targets) => (
            AttributeType::ListRelationship,
            vec![json!({"@list": targets.iter().map(target_node).collect::<Vec<_>>()})],
        ),
        CorpusContent::LanguageProperty(translations) => (
            AttributeType::LanguageProperty,
            translations
                .iter()
                .map(|translation| json!({"@language": translation.language, "@value": translation.text}))
                .collect(),
        ),
        CorpusContent::VocabProperty(term) => (AttributeType::VocabProperty, vec![json!({"@id": CorpusVocabulary::vocabulary_iri(term)})]),
        CorpusContent::ListProperty(values) => (
            AttributeType::ListProperty,
            vec![json!({"@list": values.iter().map(value_node).collect::<Vec<_>>()})],
        ),
        CorpusContent::JsonProperty(text) => (
            AttributeType::JsonProperty,
            vec![json!({"@type": "@json", "@value": serde_json::from_str::<Value>(text).expect("a corpus JSON text parses")})],
        ),
        CorpusContent::GeoProperty { longitude, latitude } => (
            AttributeType::GeoProperty,
            vec![json!({
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
            })],
        ),
    }
}

/// The node of a Relationship object.
fn target_node(target: &CorpusTarget) -> Value {
    match target {
        CorpusTarget::Identifier(identifier) => json!({"@id": identifier}),
        CorpusTarget::Entity(entity) => json!({"@id": corpus_id(*entity)}),
    }
}

/// The node of one value.
fn value_node(value: &CorpusValue) -> Value {
    match value {
        CorpusValue::Integer(number) => json!({"@value": number}),
        CorpusValue::Text(text) => json!({"@value": text}),
        CorpusValue::Boolean(flag) => json!({"@value": flag}),
        CorpusValue::Declared { datatype, lexical } => json!({"@value": lexical, "@type": datatype_iri(*datatype)}),
        CorpusValue::Members(members) => Value::Object(
            members
                .iter()
                .map(|(term, values)| (CorpusVocabulary::attribute_iri(term), Value::Array(values.iter().map(value_node).collect())))
                .collect(),
        ),
    }
}

/// The IRI a literal of `datatype` is typed with.
const fn datatype_iri(datatype: CorpusDatatype) -> &'static str {
    match datatype {
        CorpusDatatype::Date => "https://uri.etsi.org/ngsi-ld/Date",
        CorpusDatatype::DateTime => "https://uri.etsi.org/ngsi-ld/DateTime",
        CorpusDatatype::Time => "https://uri.etsi.org/ngsi-ld/Time",
        CorpusDatatype::Double => "http://www.w3.org/2001/XMLSchema#double",
    }
}
