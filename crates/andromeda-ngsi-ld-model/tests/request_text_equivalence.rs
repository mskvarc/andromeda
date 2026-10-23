//! A request body decoded straight from its text is the body the parsed-tree
//! route reads: the tree parsed, its numbers respelled, its `@context`
//! removed, and the domain type read from the tree by the compact model's
//! reading of a Context Producer's node.
//!
//! The corpus is every entity and entity patch body the integration suites
//! write as a plain JSON literal, kept in the spelling they wrote it in, and
//! the annex C corpus; the cases below it cover what the corpus leaves out.

use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::{Entity, EntityPatch},
    request_body::RequestBody,
};
use andromeda_ngsi_ld_number::json_value::text_admission::admit_json_text;
use andromeda_ngsi_ld_representation::{compact_node::produced_node::ProducedNode, core_term_aliases::CoreTermAliases};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json, value::RawValue};
use std::{collections::BTreeMap, fmt::Debug};

/// The request body corpus, spelled as the suites write it.
const CORPUS: &str = include_str!("fixtures/request-bodies.json");

/// One body of the corpus.
#[derive(Debug, Deserialize)]
struct CorpusBody {
    /// The suite file the body comes from.
    source: String,
    /// `entity`, `entity-array` or `patch`.
    kind: String,
    /// The body as written.
    text: String,
}

/// A body the compact model reads from a node.
trait ReadFromNode: Sized {
    /// Read the body from a Context Producer's parsed document.
    fn read(document: &Value) -> Result<Self, String>;
}

impl ReadFromNode for Entity {
    fn read(document: &Value) -> Result<Entity, String> {
        let text = RawValue::from_string(document.to_string()).map_err(|error| error.to_string())?;
        <Entity>::from_node(ProducedNode::new(JsonPiece::new(&text))).map_err(|error| error.to_string())
    }
}

impl ReadFromNode for EntityPatch {
    fn read(document: &Value) -> Result<EntityPatch, String> {
        let text = RawValue::from_string(document.to_string()).map_err(|error| error.to_string())?;
        <EntityPatch>::from_node(ProducedNode::new(JsonPiece::new(&text))).map_err(|error| error.to_string())
    }
}

/// Decode `text` the parsed-tree way.
fn through_tree<T: ReadFromNode>(text: &str) -> Result<T, String> {
    let mut document: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    admit_json_text(text.as_bytes()).map_err(|error| error.to_string())?;
    if let Value::Object(members) = &mut document {
        members.remove("@context");
    }
    T::read(&document)
}

/// Decode `text` straight from the text, its numbers checked first as the
/// transport checks them.
fn through_text<T: RequestBody>(text: &str) -> Result<T, String> {
    admit_json_text(text.as_bytes()).map_err(|error| error.to_string())?;
    T::from_request_text(text.as_bytes(), CoreTermAliases::keyword_spellings()).map_err(|error| error.to_string())
}

/// What an attribute member decoded into: its variant, and the same of every
/// sub-attribute of every instance.
fn member_shape(member: &AttributeValue) -> Value {
    let variant = match member {
        AttributeValue::Single(_) => "single",
        AttributeValue::Multiple(_) => "multiple",
        AttributeValue::NgsiLdNull => "null",
    };
    let instances: Vec<Value> = member.instances().map(instance_shape).collect();
    json!({"variant": variant, "instances": instances})
}

/// The family and sub-attribute shapes of one instance.
fn instance_shape(instance: &Attribute) -> Value {
    let subs: Map<String, Value> = instance
        .common()
        .sub_attributes
        .iter()
        .map(|(name, member)| (name.to_string(), member_shape(member)))
        .collect();
    json!({"family": instance.attribute_type().to_string(), "subs": subs})
}

/// What a decoded body is, member order aside: what it renders as, and the
/// shape of every attribute member.
type Shape = (Value, BTreeMap<String, Value>);

/// What an entity decoded into.
fn entity_shape(entity: &Entity) -> Result<Shape, String> {
    let members = entity.attribute_members().map(|(name, member)| (name.to_owned(), member_shape(member)));
    Ok((rendered(entity)?, members.collect()))
}

/// What a patch decoded into.
fn patch_shape(patch: &EntityPatch) -> Result<Shape, String> {
    let members = [&patch.location, &patch.observation_space, &patch.operation_space]
        .into_iter()
        .zip(["location", "observationSpace", "operationSpace"])
        .filter_map(|(member, name)| member.as_ref().map(|member| (name.to_owned(), member_shape(member))))
        .chain(patch.attrs.iter().map(|(name, member)| (name.to_string(), member_shape(member))));
    Ok((rendered(patch)?, members.collect()))
}

/// `value` rendered as JSON.
fn rendered<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

/// How the two routes disagree on `text` decoded as a `T`, if they do.
fn disagreement<T: RequestBody + ReadFromNode + Debug>(text: &str, source: &str, shape: impl Fn(&T) -> Result<Shape, String>) -> Option<String> {
    match (through_text::<T>(text), through_tree::<T>(text)) {
        (Ok(streamed), Ok(parsed)) => match (shape(&streamed), shape(&parsed)) {
            (Ok(streamed), Ok(parsed)) if streamed == parsed => None,
            (streamed, parsed) => Some(format!("{source}: {text}\nfrom the text: {streamed:?}\nfrom the tree: {parsed:?}")),
        },
        (Err(_), Err(_)) => None,
        (streamed, parsed) => Some(format!("{source}: {text}\nfrom the text: {streamed:?}\nfrom the tree: {parsed:?}")),
    }
}

/// How the two routes disagree on `text` as an entity and as a patch.
fn disagreements(text: &str, source: &str, kinds: Kinds) -> Vec<String> {
    let as_entity = match kinds {
        Kinds::EntityAndPatch | Kinds::Entity => disagreement::<Entity>(text, source, entity_shape),
        Kinds::Patch => None,
    };
    let as_patch = match kinds {
        Kinds::EntityAndPatch | Kinds::Patch => disagreement::<EntityPatch>(text, source, patch_shape),
        Kinds::Entity => None,
    };
    as_entity.into_iter().chain(as_patch).collect()
}

/// Which domain types a body is decoded as.
#[derive(Clone, Copy, Debug)]
enum Kinds {
    /// As an entity and as a patch.
    EntityAndPatch,
    /// As an entity only.
    Entity,
    /// As a patch only.
    Patch,
}

#[test]
fn every_corpus_body_decodes_from_its_text_as_from_its_tree() {
    let corpus: Vec<CorpusBody> = serde_json::from_str(CORPUS).expect("the corpus is JSON");
    assert!(corpus.len() > 50, "the corpus holds {} bodies", corpus.len());
    let mut found = Vec::new();
    for body in &corpus {
        match body.kind.as_str() {
            "entity" => found.extend(disagreements(&body.text, &body.source, Kinds::EntityAndPatch)),
            "patch" => found.extend(disagreements(&body.text, &body.source, Kinds::Patch)),
            "entity-array" => {
                let items: Vec<&RawValue> = serde_json::from_str(&body.text).expect("an array of entities");
                for item in items {
                    found.extend(disagreements(item.get(), &body.source, Kinds::Entity));
                }
            }
            other => panic!("{}: unknown corpus kind {other}", body.source),
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// Bodies the corpus leaves out: number spellings, members in every order,
/// geometries that are refused or carry foreign members, members that are no
/// instance, and the clause 5.2.4 geo-properties.
const CASES: [&str; 22] = [
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[21.5,-7,0,18446744073709551616,0.100000000000000005,1.50,1E+20,-0]}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"value":{"a":[1.0,{"b":2.50}]},"type":"Property","unitCode":"CEL","observedAt":"2024-01-01T00:00:00Z"}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","location":{"value":{"coordinates":[13.4,52.5],"type":"Point"},"type":"GeoProperty"}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","location":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[[[0,0],[4.0,0],[4,4,12.5],[0,0]]]},"accuracy":{"type":"Property","value":1.0}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[[[0,0],[4,0],[0,0]]]}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":{"type":"Point","coordinates":[181,0]}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1e400,0]}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":{"@context":{},"type":"Point","coordinates":[1,2]}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1,2],"title":"foreign"}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":"{\"type\":\"Point\",\"coordinates\":[1,2]}"}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1,2]},"previousValue":null}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","area":{"type":"GeoProperty"}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","p":{"type":"Property","value":{"type":"Point","coordinates":[1.5,2.5]}}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","multi":[{"type":"Property","value":1.0},{"type":"Property","value":2,"datasetId":"urn:ngsi-ld:Dataset:b"}]}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","mixed":[{"type":"Property","value":1},{"no":"type"}],"empty":[]}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","concise":21.50,"keyValue":"text","flag":true,"nothing":null,"object":{"value":1.0}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","location":null,"observationSpace":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1,2]}}}"#,
    r#"{"@context":"https://example.org/context.jsonld","id":"urn:ngsi-ld:T:1","type":["T","U"],"scope":["/a","/b"],"expiresAt":"2030-01-01T00:00:00Z"}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","r":{"type":"Relationship","object":"urn:ngsi-ld:T:2","since":{"type":"Property","value":3.0}},"l":{"type":"ListProperty","valueList":[1.0,"a",{"b":2.0}]}}"#,
    r#"{"id":"urn:ngsi-ld:T:1","type":"T","j":{"type":"JsonProperty","json":{"n":1E+2}},"v":{"type":"VocabProperty","vocab":"x"},"lang":{"type":"LanguageProperty","languageMap":{"en":"a"}}}"#,
    r#"{"type":"T","n":{"type":"Property","value":1}}"#,
    r#"{"id":"not a uri","type":"T"}"#,
];

#[test]
fn every_case_decodes_from_its_text_as_from_its_tree() {
    let found: Vec<String> = CASES.iter().flat_map(|text| disagreements(text, "case", Kinds::EntityAndPatch)).collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// A number held as JSON takes the spelling the parsed-tree route gives it.
#[test]
fn a_number_held_as_json_is_respelled() {
    let entity: Entity = through_text(r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[1.50,1E+2,-0]},"raw":2.0}"#).expect("an entity");
    let rendered = serde_json::to_string(&entity).expect("an entity renders");
    assert!(rendered.contains("[1.5,100,0]"), "{rendered}");
    assert!(rendered.contains(r#""raw":{"type":"Property","value":2}"#), "{rendered}");
}
