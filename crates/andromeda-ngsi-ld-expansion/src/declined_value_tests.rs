//! A write the typed walk declines is counted against the entity value limit
//! before the JSON-LD processor expands it (clause 6.3.2). A body context that
//! gives an attribute term a property-scoped context which does not propagate
//! makes the walk decline the attribute before it counts any of its members, so
//! only the count taken from the typed model stands between the processor and
//! the whole write.

use crate::{
    decline::Decline,
    entity_expansion::EntityExpansion,
    error::ExpansionError,
    expansion_fixture::{admitted, engine},
    model_census::ModelCensus,
    normalization::Normalization,
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::{member_reading::MemberReading, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{compact_node::produced_node::ProducedNode, payload_budget::PayloadBudgetExceeded};
use serde_json::{Map, Value, json, value::RawValue};
use std::{fmt::Debug, num::NonZeroUsize};

/// A limit of `values` values.
fn limit(values: usize) -> JsonValueLimit {
    JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive limit"))
}

/// A body context defining each of `terms` as a term whose scoped context does
/// not propagate, which the walk declines wherever the term is written.
fn declining_context(terms: &[&str]) -> Value {
    let definitions: Map<String, Value> = terms
        .iter()
        .map(|term| {
            let iri = if term.contains("://") {
                (*term).to_owned()
            } else {
                format!("https://example.org/declined/{term}")
            };
            ((*term).to_owned(), json!({"@id": iri, "@context": {"@propagate": false}}))
        })
        .collect();
    Value::Array(vec![Value::Object(definitions)])
}

/// An entity carrying `members` beside its identifier and type, under a
/// context declining each member's term.
fn entity(members: &Value) -> Value {
    let members = members.as_object().expect("members");
    let terms: Vec<&str> = members.keys().map(String::as_str).collect();
    let mut body = json!({"id": "urn:ngsi-ld:Car:1", "type": "Car", "@context": declining_context(&terms)});
    body.as_object_mut().expect("an object").extend(members.clone()); // clone: the fixture members are borrowed
    body
}

/// Assert a clause 6.3.2 refusal naming the limit of `values` values.
fn assert_refused<T: Debug>(expanded: Result<T, ExpansionError>, values: usize) {
    match expanded {
        Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit: refused })) => pretty_assertions::assert_eq!(refused, limit(values)),
        other => panic!("expected the value limit of {values} to refuse the write, got {other:?}"),
    }
}

/// Expand the entity `body` bounded by `values` values, or by none.
async fn expand_entity(body: Value, values: Option<usize>) -> Result<(), ExpansionError> {
    let (body, spec) = admitted(body);
    let engine = engine();
    let context = engine.expansion_context(&spec).await.expect("the declining context processes");
    let entity: Entity = serde_json::from_value(body).expect("a typed entity");
    let expansion = EntityExpansion::from_context(&engine, context);
    let expansion = values.map_or(expansion.clone(), |values| expansion.bounded_by(limit(values))); // clone: the unbounded expansion is the same one
    expansion.entity(&entity, Normalization::Entity).await.map(|_document| ())
}

/// Assert that `body` is admitted at `values` values and refused at one fewer.
async fn assert_counts(body: &Value, values: usize) {
    expand_entity(body.clone(), Some(values)).await.expect("the write is within the limit"); // clone: the body is expanded under two limits
    assert_refused(expand_entity(body.clone(), Some(values - 1)).await, values - 1); // clone: as above
}

#[tokio::test]
async fn the_walk_declines_an_attribute_whose_scoped_context_does_not_propagate() {
    let (body, spec) = admitted(entity(&json!({"speed": {"type": "Property", "value": 1}})));
    let context = engine().expansion_context(&spec).await.expect("the context processes");
    let entity: Entity = serde_json::from_value(body).expect("a typed entity");
    let walked = TypedExpander::new(&context)
        .bounded_by(limit(1))
        .entity(&entity, Normalization::Entity)
        .expect("a declined walk");
    pretty_assertions::assert_eq!(walked, TypedExpansion::Declined(Decline::NonPropagatedContext));
}

#[tokio::test]
async fn a_declined_property_value_counts_each_of_its_items() {
    let numbers: Vec<u32> = (0..20).collect();
    assert_counts(&entity(&json!({"speed": {"type": "Property", "value": numbers}})), 20).await;
}

#[tokio::test]
async fn a_declined_value_counts_the_terms_of_a_structured_value() {
    assert_counts(&entity(&json!({"speed": {"type": "Property", "value": {"a": [1, 2, 3], "b": "x"}}})), 5).await;
}

#[tokio::test]
async fn a_declined_value_list_counts_its_items() {
    assert_counts(&entity(&json!({"speeds": {"type": "ListProperty", "valueList": [1, 2, 3]}})), 3).await;
}

#[tokio::test]
async fn a_declined_relationship_counts_each_target() {
    assert_counts(
        &entity(&json!({"owner": {"type": "Relationship", "object": ["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2", "urn:ngsi-ld:A:3"]}})),
        3,
    )
    .await;
}

#[tokio::test]
async fn a_declined_list_relationship_counts_the_list_and_each_entry() {
    assert_counts(
        &entity(&json!({"friends": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"]}})),
        3,
    )
    .await;
}

#[tokio::test]
async fn a_declined_vocab_property_counts_each_term() {
    assert_counts(&entity(&json!({"kind": {"type": "VocabProperty", "vocab": ["a", "b", "c"]}})), 3).await;
}

#[tokio::test]
async fn a_declined_language_property_counts_the_map_and_each_string() {
    assert_counts(
        &entity(&json!({"name": {"type": "LanguageProperty", "languageMap": {"en": ["a", "b"], "fr": "c"}}})),
        4,
    )
    .await;
}

#[tokio::test]
async fn the_values_of_a_declined_sub_attribute_count_with_the_rest() {
    let body =
        entity(&json!({"speed": {"type": "Property", "value": 1, "source": {"type": "Relationship", "object": ["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"]}}}));
    assert_counts(&body, 3).await;
}

#[tokio::test]
async fn the_attributes_of_a_declined_entity_count_together() {
    let body = entity(&json!({
        "speed": {"type": "Property", "value": [1, 2]},
        "owner": {"type": "Relationship", "object": "urn:ngsi-ld:A:1"}
    }));
    assert_counts(&body, 3).await;
}

#[tokio::test]
async fn a_declined_write_bounded_by_no_limit_counts_nothing() {
    let numbers: Vec<u32> = (0..20).collect();
    expand_entity(entity(&json!({"speed": {"type": "Property", "value": numbers}})), None)
        .await
        .expect("an unbounded expansion refuses no value");
}

#[tokio::test]
async fn a_declined_merge_fragment_counts_its_values() {
    let body = entity(&json!({"speed": {"type": "Property", "value": [1, 2, 3]}}));
    let (body, spec) = admitted(body);
    let engine = engine();
    let context = engine.expansion_context(&spec).await.expect("the context processes");
    let patch: EntityPatch<MergeMember> = serde_json::from_value(body).expect("a typed fragment");
    let expansion = EntityExpansion::from_context(&engine, context);
    expansion
        .clone() // clone: the fragment is expanded under two limits
        .bounded_by(limit(3))
        .entity_patch(&patch, Normalization::MemberDeletions)
        .await
        .expect("the fragment is within the limit");
    assert_refused(expansion.bounded_by(limit(2)).entity_patch(&patch, Normalization::MemberDeletions).await, 2);
}

#[tokio::test]
async fn a_declined_attribute_fragment_counts_its_values() {
    let name = "https://example.org/speed";
    let (_, spec) = admitted(json!({"@context": declining_context(&[name])}));
    let engine = engine();
    let context = engine.expansion_context(&spec).await.expect("the context processes");
    let written = RawValue::from_string(json!({"type": "Property", "value": [1, 2, 3]}).to_string()).expect("a JSON text");
    let member = MergeMember::from_fragment_node("speed", ProducedNode::new(JsonPiece::new(&written))).expect("a typed fragment");
    let attribute: AttributeName = name.parse().expect("an attribute name");
    let expansion = EntityExpansion::from_context(&engine, context);
    expansion
        .clone() // clone: the fragment is expanded under two limits
        .bounded_by(limit(3))
        .attribute(&attribute, &member)
        .await
        .expect("the fragment is within the limit");
    assert_refused(expansion.bounded_by(limit(2)).attribute(&attribute, &member).await, 2);
}

#[test]
fn the_scope_of_a_fragment_counts_each_path_and_its_deletion_once() {
    let scopes: EntityPatch<MergeMember> = serde_json::from_value(json!({"scope": ["/a", "/b", "/c"]})).expect("a typed fragment");
    ModelCensus::new(limit(3)).entity_patch(&scopes).expect("three scopes within three");
    assert_refused(ModelCensus::new(limit(2)).entity_patch(&scopes), 2);
    let deleted: EntityPatch<MergeMember> = serde_json::from_value(json!({"scope": "urn:ngsi-ld:null"})).expect("a typed fragment");
    ModelCensus::new(limit(1)).entity_patch(&deleted).expect("a deletion counts once");
}
