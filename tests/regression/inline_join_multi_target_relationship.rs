//! `join=inline` over a Relationship whose `object` names several targets.
//!
//! Clause 4.5.3.2 defines `object` as "the Relationship's object represented by
//! a URI or array of URIs", and clause 4.5.23.2 nests what each URI names under
//! the Relationship's `entity` member. Clause 4.5.23.3 renders the same
//! traversal as a flat collection, so the two modes retrieve the same Linked
//! Entities and only present them differently.
//!
//! Found while replaying annex C clause C.2.2, whose Vehicle carries a
//! `passengers` Relationship with two targets: `join=flat` returned both
//! Entities while `join=inline` added no `entity` member at all, because the
//! inline rendering read `object` only as a bare string.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// One Building that a Relationship can point at.
fn target_building(identifier: &str, name: &str) -> Value {
    json!({
        "@context": [TEST_SUITE_CONTEXT],
        "id": identifier,
        "type": "Building",
        "name": {"type": "Property", "value": name}
    })
}

/// One Building whose named Relationships carry the supplied `object` members.
fn linking_building(identifier: &str, relationships: &Value) -> Value {
    let mut entity = json!({"@context": [TEST_SUITE_CONTEXT], "id": identifier, "type": "Building"});
    let members = relationships.as_object().expect("a map of Relationship members").clone();
    for (name, object) in members {
        entity[name] = json!({"type": "Relationship", "object": object});
    }
    entity
}

/// Create one Entity, asserting the broker took it.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Retrieve one Entity under the suite `@context` with the supplied query.
async fn retrieve(client: &TestClient, identifier: &str, query: &str) -> Value {
    let retrieved = client
        .get_entity(identifier, query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("entity retrieval failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieval of {identifier}: {:?}", retrieved.body);
    retrieved.json_value().expect("an Entity body")
}

/// The identifiers a `join=flat` answer holds, sorted so the comparison does not
/// depend on an order clause 5.7.2 leaves unspecified.
async fn flat_identifiers(client: &TestClient, identifier: &str, query: &str) -> Vec<String> {
    let answer = retrieve(client, identifier, query).await;
    let mut identifiers: Vec<String> = answer
        .as_array()
        .expect("join=flat answers with a collection")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    identifiers.sort();
    identifiers
}

/// The identifiers nested under one Relationship of a `join=inline` answer,
/// sorted for the same reason.
fn inline_identifiers(entity: &Value, relationship: &str) -> Vec<String> {
    let nested = &entity[relationship]["entity"];
    let mut identifiers: Vec<String> = match nested {
        Value::Array(entities) => entities.iter().filter_map(|entity| entity["id"].as_str().map(str::to_owned)).collect(),
        Value::Object(_) => nested["id"].as_str().map(str::to_owned).into_iter().collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    identifiers.sort();
    identifiers
}

/// One target, written as the bare URI of clause 4.5.3.2: `inline` nests one
/// Entity object and `flat` returns the linking Entity beside it.
#[tokio::test]
async fn regression_inline_join_multi_target_relationship_agrees_with_flat_for_one_target() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let alice = scope.id("Building:one-target-A");
    let root = scope.id("Building:one-target-root");
    create(client, &target_building(&alice, "Alice")).await;
    create(client, &linking_building(&root, &json!({"linkedTo": alice.as_str()}))).await;

    let inline = retrieve(client, &root, "join=inline&joinLevel=1").await;
    assert!(inline["linkedTo"]["entity"].is_object(), "a bare URI nests one Entity object: {inline}");
    assert_eq!(inline_identifiers(&inline, "linkedTo"), vec![alice.clone()]);

    let flat = flat_identifiers(client, &root, "join=flat&joinLevel=1").await;
    let mut expected = vec![alice.clone(), root.clone()];
    expected.sort();
    assert_eq!(flat, expected, "join=flat retrieves the same Linked Entity");

    scope.finish().await.expect("environment cleanup");
}

/// Two targets, written as the array of URIs of clause 4.5.3.2: `inline` nests
/// both in the order `object` named them, and `flat` finds the same two.
#[tokio::test]
async fn regression_inline_join_multi_target_relationship_agrees_with_flat_for_two_targets() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let alice = scope.id("Building:two-target-A");
    let bob = scope.id("Building:two-target-B");
    let root = scope.id("Building:two-target-root");
    create(client, &target_building(&alice, "Alice")).await;
    create(client, &target_building(&bob, "Bob")).await;
    create(client, &linking_building(&root, &json!({"linkedTo": [alice.as_str(), bob.as_str()]}))).await;

    let inline = retrieve(client, &root, "join=inline&joinLevel=1").await;
    let nested = inline["linkedTo"]["entity"].as_array().expect("an array of nested Entities");
    assert_eq!(nested.len(), 2, "{inline}");
    assert_eq!(nested[0]["id"], json!(alice), "the nesting keeps `object` order: {inline}");
    assert_eq!(nested[1]["id"], json!(bob), "the nesting keeps `object` order: {inline}");
    assert_eq!(nested[1]["name"]["value"], json!("Bob"), "each nested Entity is rendered in full: {inline}");

    let flat = flat_identifiers(client, &root, "join=flat&joinLevel=1").await;
    let mut expected = vec![alice.clone(), bob.clone(), root.clone()];
    expected.sort();
    assert_eq!(flat, expected, "the two modes retrieve the same Linked Entities");

    scope.finish().await.expect("environment cleanup");
}

/// One Entity carrying both spellings at once: each Relationship nests the shape
/// its own `object` was written in, and `flat` returns the union of the two.
#[tokio::test]
async fn regression_inline_join_multi_target_relationship_agrees_with_flat_for_a_mixture() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let alice = scope.id("Building:mixed-A");
    let bob = scope.id("Building:mixed-B");
    let carol = scope.id("Building:mixed-C");
    let root = scope.id("Building:mixed-root");
    create(client, &target_building(&alice, "Alice")).await;
    create(client, &target_building(&bob, "Bob")).await;
    create(client, &target_building(&carol, "Carol")).await;
    create(
        client,
        &linking_building(&root, &json!({"linkedTo": carol.as_str(), "hasNeighbours": [alice.as_str(), bob.as_str()]})),
    )
    .await;

    let inline = retrieve(client, &root, "join=inline&joinLevel=1").await;
    assert!(inline["linkedTo"]["entity"].is_object(), "{inline}");
    assert_eq!(inline_identifiers(&inline, "linkedTo"), vec![carol.clone()]);
    let mut neighbours = vec![alice.clone(), bob.clone()];
    neighbours.sort();
    assert_eq!(inline_identifiers(&inline, "hasNeighbours"), neighbours);

    let flat = flat_identifiers(client, &root, "join=flat&joinLevel=1").await;
    let mut expected = vec![alice.clone(), bob.clone(), carol.clone(), root.clone()];
    expected.sort();
    assert_eq!(flat, expected);

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.23 bounds a traversal by `joinLevel`, and a multi-target
/// Relationship is descended through exactly as a single-target one is: a second
/// hop nests under every target of the first.
#[tokio::test]
async fn regression_inline_join_multi_target_relationship_descends_a_second_join_level() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let inner = scope.id("Building:depth-inner");
    let first = scope.id("Building:depth-first");
    let second = scope.id("Building:depth-second");
    let root = scope.id("Building:depth-root");
    create(client, &target_building(&inner, "Inner")).await;
    create(client, &linking_building(&first, &json!({"linkedTo": inner.as_str()}))).await;
    create(client, &linking_building(&second, &json!({"linkedTo": inner.as_str()}))).await;
    create(client, &linking_building(&root, &json!({"hasNeighbours": [first.as_str(), second.as_str()]}))).await;

    let one_hop = retrieve(client, &root, "join=inline&joinLevel=1").await;
    let nested = one_hop["hasNeighbours"]["entity"].as_array().expect("an array of nested Entities");
    assert_eq!(nested.len(), 2, "{one_hop}");
    for neighbour in nested {
        assert!(
            neighbour["linkedTo"].get("entity").is_none(),
            "one hop must not reach the second level: {one_hop}"
        );
    }

    let two_hops = retrieve(client, &root, "join=inline&joinLevel=2").await;
    let nested = two_hops["hasNeighbours"]["entity"].as_array().expect("an array of nested Entities");
    assert_eq!(nested.len(), 2, "{two_hops}");
    for neighbour in nested {
        assert_eq!(
            neighbour["linkedTo"]["entity"]["id"],
            json!(inner),
            "each target of a multi-target Relationship is descended through: {two_hops}"
        );
    }

    let flat = flat_identifiers(client, &root, "join=flat&joinLevel=2").await;
    let mut expected = vec![inner.clone(), first.clone(), second.clone(), root.clone()];
    expected.sort();
    assert_eq!(flat, expected, "the two modes retrieve the same Linked Entities at depth two");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.4, inline Linked Entity retrieval, EXAMPLE 10: a multi-target
/// Relationship answers with the ordered array of Linked Entities in key-value
/// form, each simplified in its own right.
#[tokio::test]
async fn regression_inline_join_multi_target_relationship_simplifies_to_an_array_of_key_value_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let alice = scope.id("Building:simplified-A");
    let bob = scope.id("Building:simplified-B");
    let root = scope.id("Building:simplified-root");
    create(client, &target_building(&alice, "Alice")).await;
    create(client, &target_building(&bob, "Bob")).await;
    create(client, &linking_building(&root, &json!({"linkedTo": [alice.as_str(), bob.as_str()]}))).await;

    let simplified = retrieve(client, &root, "join=inline&joinLevel=1&options=keyValues").await;

    assert_eq!(
        simplified["linkedTo"],
        json!([
            {"id": alice, "type": "Building", "name": "Alice"},
            {"id": bob, "type": "Building", "name": "Bob"}
        ]),
        "{simplified}"
    );

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.23.1 retrieves only Relationships targeting a locally stored
/// Entity, so a target that resolves to nothing drops out of the nesting while
/// its siblings still appear, and a Relationship none of whose targets resolve
/// gains no `entity` member at all.
#[tokio::test]
async fn regression_inline_join_multi_target_relationship_nests_only_the_targets_that_resolve() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let alice = scope.id("Building:partial-A");
    let missing = scope.id("Building:partial-missing");
    let root = scope.id("Building:partial-root");
    create(client, &target_building(&alice, "Alice")).await;
    create(
        client,
        &linking_building(
            &root,
            &json!({"linkedTo": [missing.as_str(), alice.as_str()], "hasNeighbours": [missing.as_str()]}),
        ),
    )
    .await;

    let inline = retrieve(client, &root, "join=inline&joinLevel=1").await;

    assert_eq!(inline_identifiers(&inline, "linkedTo"), vec![alice.clone()], "{inline}");
    assert!(
        inline["hasNeighbours"].get("entity").is_none(),
        "a Relationship whose targets all resolve to nothing gains no member: {inline}"
    );
    // The stored Relationship keeps naming its target. Clause 4.5.3.2 admits
    // both spellings for one target and the write path collapses a one-element
    // array to the bare URI, so the assertion is on the target rather than on
    // which of the two spellings came back.
    let unresolved = &inline["hasNeighbours"]["object"];
    let named: Vec<&str> = match unresolved {
        Value::Array(uris) => uris.iter().filter_map(Value::as_str).collect(),
        single @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => single.as_str().into_iter().collect(),
    };
    assert_eq!(named, vec![missing.as_str()], "the unresolved Relationship keeps its URI: {inline}");

    scope.finish().await.expect("environment cleanup");
}
