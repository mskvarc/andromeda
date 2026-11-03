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

/// The members a count never reaches: the instance's own scalar members, a
/// JSON literal and the identifier and type of the entity are the census's to
/// count.
#[tokio::test]
async fn the_members_the_walk_does_not_count_are_left_to_the_census() {
    let body = entity(&json!({
        "speed": {"type": "Property", "value": 1, "unitCode": "KMH", "observedAt": "2024-01-01T00:00:00Z"},
        "config": {"type": "JsonProperty", "json": {"a": [1, 2, 3]}}
    }));
    pretty_assertions::assert_eq!(walk_entity(body, 1).await.ok(), Some(true), "one value below a carrier");
}

/// A Relationship `object` counts one value per target, a single target
/// written bare included.
#[tokio::test]
async fn a_relationship_object_counts_each_target() {
    assert_counts(
        &entity(&json!({"owner": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2", "urn:ngsi-ld:Person:3"]}})),
        3,
    )
    .await;
    assert_counts(
        &entity(&json!({"owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1"}, "speed": {"type": "Property", "value": 1}})),
        2,
    )
    .await;
}

/// A `ListRelationship` `objectList` counts the list and each entry: an entry
/// expands to two items and is counted once.
#[tokio::test]
async fn a_list_relationship_counts_the_list_and_each_entry() {
    assert_counts(
        &entity(&json!({"friends": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2", "urn:ngsi-ld:Person:3"]}})),
        4,
    )
    .await;
}

/// A `VocabProperty` `vocab` counts one value per term.
#[tokio::test]
async fn a_vocab_property_counts_each_term() {
    assert_counts(&entity(&json!({"kind": {"type": "VocabProperty", "vocab": ["a", "b", "c", "d"]}})), 4).await;
}

/// A `LanguageProperty` `languageMap` counts the map and each string over
/// every tag, an array of strings under one tag included.
#[tokio::test]
async fn a_language_property_counts_the_map_and_each_string() {
    assert_counts(
        &entity(&json!({"name": {"type": "LanguageProperty", "languageMap": {"en": "a", "fr": ["b", "c"], "@none": "d"}}})),
        5,
    )
    .await;
    assert_counts(
        &entity(&json!({"name": {"type": "LanguageProperty", "languageMap": {"en": ["a", "b", "c"]}}})),
        4,
    )
    .await;
}

/// The entity `scope` counts one value per scope.
#[tokio::test]
async fn an_entity_scope_counts_each_scope() {
    assert_counts(&entity(&json!({"scope": ["/a", "/b", "/c"]})), 3).await;
    assert_counts(&entity(&json!({"scope": "/a", "speed": {"type": "Property", "value": 1}})), 2).await;
}

/// An entity whose members add up to the limit exactly is admitted, and one
/// value fewer refuses it: the early count is the census's count, not the
/// count of the items the expansion makes.
#[tokio::test]
async fn the_members_count_together_with_the_values_up_to_the_limit() {
    let body = entity(&json!({
        "scope": ["/a", "/b"],
        "speed": {"type": "Property", "value": 1},
        "owner": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2"]},
        "friends": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:3", "urn:ngsi-ld:Person:4"]},
        "kind": {"type": "VocabProperty", "vocab": ["a", "b"]},
        "name": {"type": "LanguageProperty", "languageMap": {"en": "a", "fr": ["b", "c"]}}
    }));
    assert_counts(&body, 2 + 1 + 2 + 3 + 2 + 4).await;
}

/// A member below a sub-attribute is counted like one of the entity.
#[tokio::test]
async fn a_member_of_a_sub_attribute_is_counted() {
    let body = entity(
        &json!({"speed": {"type": "Property", "value": 1, "source": {"type": "Relationship", "object": ["urn:ngsi-ld:Sensor:1", "urn:ngsi-ld:Sensor:2"]}}}),
    );
    assert_counts(&body, 3).await;
}

/// A merge fragment counts these members as an entity does (clause 5.6.17).
#[tokio::test]
async fn a_merge_fragment_counts_its_relationship_vocab_language_and_scope_members() {
    let (body, context) = under_terms(json!({
        "scope": ["/a", "/b"],
        "owner": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2"]},
        "friends": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:3"]},
        "kind": {"type": "VocabProperty", "vocab": ["a", "b"]},
        "name": {"type": "LanguageProperty", "languageMap": {"en": "a", "fr": ["b", "c"]}}
    }))
    .await;
    let patch: EntityPatch<MergeMember> = serde_json::from_value(body).expect("a typed fragment");
    let expander = TypedExpander::new(&context);
    let values = 2 + 2 + 2 + 2 + 4;
    assert!(expander.bounded_by(limit(values)).entity_patch(&patch, Normalization::MemberDeletions).is_ok());
    assert_refused(
        expander.bounded_by(limit(values - 1)).entity_patch(&patch, Normalization::MemberDeletions),
        values - 1,
    );
}

/// An Attribute Fragment counts these members as the attribute of an entity
/// does (clauses 5.6.4, 5.6.19).
#[tokio::test]
async fn an_attribute_fragment_counts_its_relationship_object() {
    let (_, context) = under_terms(json!({})).await;
    let written =
        RawValue::from_string(json!({"type": "Relationship", "object": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2"]}).to_string()).expect("a JSON text");
    let member = MergeMember::from_fragment_node("owner", ProducedNode::new(JsonPiece::new(&written))).expect("a typed fragment");
    let attribute: AttributeName = "https://example.org/terms#owner".parse().expect("an attribute name");
    let expander = TypedExpander::new(&context);
    assert!(expander.bounded_by(limit(2)).attribute(&attribute, &member).is_ok());
    assert_refused(expander.bounded_by(limit(1)).attribute(&attribute, &member), 1);
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
