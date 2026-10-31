//! The typed walk bounded by the entity value limit: what it counts below a
//! Property `value` and a `valueList`, and the clause 6.3.2 refusal it
//! answers once the count passes the limit.

use crate::{
    error::ExpansionError,
    expansion_fixture::{TERMS, admitted, engine},
    normalization::Normalization,
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_jsonld::engine::expansion_context::ExpansionContext;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::{member_reading::MemberReading, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{compact_node::produced_node::ProducedNode, payload_budget::PayloadBudgetExceeded};
use serde_json::{Value, json, value::RawValue};
use std::{fmt::Debug, num::NonZeroUsize, sync::Arc};

/// A limit of `values` values.
fn limit(values: usize) -> JsonValueLimit {
    JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive limit"))
}

/// `body` under the test terms, with the processed context it names.
async fn under_terms(mut body: Value) -> (Value, Arc<ExpansionContext>) {
    body.as_object_mut().expect("an entity body").insert("@context".to_owned(), json!(TERMS));
    let (body, spec) = admitted(body);
    let context = engine().expansion_context(&spec).await.expect("the test context processes");
    (body, context)
}

/// What the walk bounded by `values` makes of the entity `body`: `Ok(true)`
/// for an expanded document, `Ok(false)` for a declined one.
async fn walk_entity(body: Value, values: usize) -> Result<bool, ExpansionError> {
    let (body, context) = under_terms(body).await;
    let entity: Entity = serde_json::from_value(body).expect("a typed entity");
    let walked = TypedExpander::new(&context).bounded_by(limit(values)).entity(&entity, Normalization::Entity)?;
    Ok(matches!(walked, TypedExpansion::Expanded(_)))
}

/// Assert that `body` expands within `values` values and is refused under one
/// fewer, naming that limit.
async fn assert_counts(body: &Value, values: usize) {
    pretty_assertions::assert_eq!(walk_entity(body.clone(), values).await.ok(), Some(true), "{values} values admit {body}"); // clone: the body is walked under two limits
    assert_refused(walk_entity(body.clone(), values - 1).await, values - 1); // clone: as above
}

/// Assert a clause 6.3.2 refusal naming the limit of `values` values.
fn assert_refused<T: Debug>(walked: Result<T, ExpansionError>, values: usize) {
    match walked {
        Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit: refused })) => pretty_assertions::assert_eq!(refused, limit(values)),
        other => panic!("expected the value limit of {values} to refuse the walk, got {other:?}"),
    }
}

/// An entity carrying `members` beside its identifier and type.
fn entity(members: &Value) -> Value {
    let mut body = json!({"id": "urn:ngsi-ld:Car:1", "type": "Car"});
    body.as_object_mut().expect("an object").extend(members.as_object().expect("members").clone()); // clone: the fixture members are borrowed
    body
}

/// Every number of a Property value array is one value.
#[tokio::test]
async fn a_property_value_array_counts_each_of_its_items() {
    assert_counts(&entity(&json!({"speed": {"type": "Property", "value": [1, 2, 3, 4, 5]}})), 5).await;
}

/// A structured value counts its node and every value below it, whatever
/// nests them.
#[tokio::test]
async fn a_structured_value_counts_its_node_and_every_member_value() {
    let structured = entity(&json!({"speed": {"type": "Property", "value": {"a": 1, "b": [2, 3], "c": {"d": true}}}}));
    assert_counts(&structured, 6).await;
}

/// A `@list` term makes one list object of its items, which counts besides
/// them, as the census counts a list.
#[tokio::test]
async fn a_list_object_inside_a_value_counts_besides_its_items() {
    assert_counts(&entity(&json!({"speed": {"type": "Property", "value": {"tags": [1, 2]}}})), 4).await;
}

/// A term coerced to `@json` keeps its value verbatim, one JSON literal
/// whatever it holds.
#[tokio::test]
async fn a_json_literal_inside_a_value_counts_once() {
    assert_counts(&entity(&json!({"speed": {"type": "Property", "value": {"settings": {"x": [1, 2, 3, 4]}}}})), 2).await;
}

/// A language map inside a value counts each string it tags.
#[tokio::test]
async fn a_language_map_inside_a_value_counts_each_string() {
    assert_counts(
        &entity(&json!({"speed": {"type": "Property", "value": {"label": {"en": "a", "fr": ["b", "c"]}}}})),
        4,
    )
    .await;
}

/// A `valueList` counts its items; the list object the census adds for it is
/// left to the census.
#[tokio::test]
async fn a_value_list_counts_its_items() {
    assert_counts(&entity(&json!({"speed": {"type": "ListProperty", "valueList": [1, [2, 3], {"a": 4}]}})), 6).await;
}

/// The count runs over the whole document: every attribute, every instance
/// and every sub-attribute adds its values to one count.
#[tokio::test]
async fn the_values_of_every_attribute_and_sub_attribute_count_together() {
    let body = entity(&json!({
        "speed": [
            {"type": "Property", "value": [1, 2], "datasetId": "urn:ngsi-ld:Dataset:a"},
            {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b", "accuracy": {"type": "Property", "value": [4, 5]}}
        ],
        "category": {"type": "Property", "value": 6}
    }));
    assert_counts(&body, 6).await;
}

/// Only the values below a value carrier are counted: the entity, its
/// instances and their other members are the census's to count.
#[tokio::test]
async fn the_members_around_a_value_are_not_counted() {
    let body = entity(&json!({
        "scope": ["/a", "/b", "/c"],
        "speed": {"type": "Property", "value": 1, "unitCode": "KMH", "observedAt": "2024-01-01T00:00:00Z"},
        "owner": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2"]},
        "name": {"type": "LanguageProperty", "languageMap": {"en": "a", "fr": "b"}},
        "kind": {"type": "VocabProperty", "vocab": ["a", "b"]},
        "config": {"type": "JsonProperty", "json": {"a": [1, 2, 3]}}
    }));
    pretty_assertions::assert_eq!(walk_entity(body, 1).await.ok(), Some(true), "one value below a carrier");
}

/// An expander bounded by no limit counts nothing.
#[tokio::test]
async fn an_unbounded_walk_refuses_no_value() {
    let (body, context) = under_terms(entity(&json!({"speed": {"type": "Property", "value": [1, 2, 3, 4, 5]}}))).await;
    let entity: Entity = serde_json::from_value(body).expect("a typed entity");
    let walked = TypedExpander::new(&context).entity(&entity, Normalization::Entity).expect("an unbounded walk");
    assert!(matches!(walked, TypedExpansion::Expanded(_)), "{walked:?}");
}

/// A merge fragment's values are counted as an entity's are (clause 5.6.17).
#[tokio::test]
async fn a_merge_fragment_counts_its_values() {
    let (body, context) = under_terms(json!({"speed": {"type": "Property", "value": [1, 2, 3]}})).await;
    let patch: EntityPatch<MergeMember> = serde_json::from_value(body).expect("a typed fragment");
    let expander = TypedExpander::new(&context);
    assert!(expander.bounded_by(limit(3)).entity_patch(&patch, Normalization::MemberDeletions).is_ok());
    assert_refused(expander.bounded_by(limit(2)).entity_patch(&patch, Normalization::MemberDeletions), 2);
}

/// An Attribute Fragment of clauses 5.6.4 and 5.6.19 is counted as the
/// attribute of an entity is.
#[tokio::test]
async fn an_attribute_fragment_counts_its_values() {
    let (_, context) = under_terms(json!({})).await;
    let written = RawValue::from_string(json!({"type": "Property", "value": [1, 2, 3]}).to_string()).expect("a JSON text");
    let member = MergeMember::from_fragment_node("speed", ProducedNode::new(JsonPiece::new(&written))).expect("a typed fragment");
    let attribute: AttributeName = "https://example.org/terms#speed".parse().expect("an attribute name");
    let expander = TypedExpander::new(&context);
    assert!(expander.bounded_by(limit(3)).attribute(&attribute, &member).is_ok());
    assert_refused(expander.bounded_by(limit(2)).attribute(&attribute, &member), 2);
}
