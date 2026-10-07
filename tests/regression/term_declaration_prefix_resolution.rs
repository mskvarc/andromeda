//! A `@context` term definition whose IRI is a compact IRI, on every write path.
//!
//! JSON-LD 1.1 clause 4.1.2 IRI-expands a term definition's `@id` against the
//! active context, so `{"example": "http://example.org/", "Place":
//! "example:Place"}` defines `Place` as `http://example.org/Place`. Clause 5.5.7
//! hands the single write of clause 5.6.1 and the batch writes of clauses 5.6.7
//! to 5.6.9 and 5.6.20 the same `@context`, so all five have to reach the same
//! expanded IRIs -- an Entity stored under a different one answers 404 to a
//! retrieval phrased in the very terms that wrote it.
//!
//! This module covers the `@context` carried inline in the payload, and the
//! three neighbouring declaration shapes: a prefix chain, an `@id` that is
//! already absolute, and a prefix no context declares.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The namespace every term below resolves into.
const NAMESPACE: &str = "http://example.org/";
/// The expanded IRI of the `Place` type.
const EXPANDED_TYPE: &str = "http://example.org/Place";
/// The expanded IRI of the `address` Property.
const EXPANDED_ADDRESS: &str = "http://example.org/address";

/// A `@context` that declares a prefix and defines two terms through it.
fn prefixed_context() -> Value {
    json!({"example": NAMESPACE, "Place": "example:Place", "address": "example:address"})
}

/// One Place Entity carrying the `@context` that defines its terms.
fn place(identifier: &str, context: &Value) -> Value {
    json!({
        "@context": context,
        "id": identifier,
        "type": "Place",
        "address": {"type": "Property", "value": "Pariser Platz, Berlin, Germany"}
    })
}

/// One Place Entity with no attributes, used to seed the two batch verbs that
/// modify an Entity rather than create one.
fn bare_place(identifier: &str, context: &Value) -> Value {
    json!({"@context": context, "id": identifier, "type": "Place"})
}

/// The stored Entity as the broker holds it, with no `@context` on the
/// retrieval so every user term shows the IRI it actually expanded to.
async fn stored(client: &TestClient, identifier: &str) -> Value {
    let retrieved = client
        .get_entity(identifier, "", "application/json", None)
        .await
        .expect("entity retrieval failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieval of {identifier}: {:?}", retrieved.body);
    retrieved.json_value().expect("an Entity body")
}

/// Create one Entity through `POST /entities`, asserting the broker took it.
async fn create_one(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("single creation failed");
    assert_eq!(created.status, StatusCode::CREATED, "single creation: {:?}", created.body);
}

/// Clause 5.6.7's batch create against clause 5.6.1's single create, plus the
/// three batch verbs of clauses 5.6.8, 5.6.9 and 5.6.20 that share the same
/// preparation. Every one of them has to store the IRI clause 4.1.2 resolves,
/// and the Entity has to be reachable under the `@context` that wrote it.
#[tokio::test]
async fn regression_term_declaration_prefix_resolution_agrees_across_every_write_path() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let single = scope.id("Place:prefix-single");
    create_one(client, &place(&single, &prefixed_context())).await;

    let created = scope.id("Place:prefix-created");
    let response = client
        .batch_create(&json!([place(&created, &prefixed_context())]))
        .await
        .expect("batch create failed");
    assert_eq!(response.status, StatusCode::CREATED, "batch create: {:?}", response.body);

    let upserted = scope.id("Place:prefix-upserted");
    let response = client
        .batch_upsert(&json!([place(&upserted, &prefixed_context())]))
        .await
        .expect("batch upsert failed");
    assert_eq!(response.status, StatusCode::CREATED, "batch upsert: {:?}", response.body);

    // Update and merge modify an Entity that already exists, so each is seeded
    // through the single path and then writes the Attribute whose name is under
    // test.
    let updated = scope.id("Place:prefix-updated");
    create_one(client, &bare_place(&updated, &prefixed_context())).await;
    let response = client
        .batch_update(&json!([place(&updated, &prefixed_context())]))
        .await
        .expect("batch update failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "batch update: {:?}", response.body);

    let merged = scope.id("Place:prefix-merged");
    create_one(client, &bare_place(&merged, &prefixed_context())).await;
    let response = client
        .batch_merge(&json!([place(&merged, &prefixed_context())]))
        .await
        .expect("batch merge failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "batch merge: {:?}", response.body);

    for identifier in [&single, &created, &upserted, &updated, &merged] {
        let entity = stored(client, identifier).await;
        assert_eq!(entity["type"], json!(EXPANDED_TYPE), "stored type of {identifier}: {entity}");
        assert!(entity.get(EXPANDED_ADDRESS).is_some(), "stored Attribute name of {identifier}: {entity}");
        assert!(
            entity.get("example:address").is_none(),
            "no write path may store the unresolved compact IRI: {entity}"
        );
    }

    // Clause C.7: the terms a client reads an Entity back with are the ones it
    // names on the retrieval. An Entity the broker acknowledged creating and
    // then cannot find under the `@context` that wrote it has been lost.
    for identifier in [&single, &created, &upserted, &updated, &merged] {
        let retrieved = client
            .get_entity(identifier, "", "application/json", None)
            .await
            .expect("entity retrieval failed");
        assert_eq!(retrieved.status, StatusCode::OK, "{identifier} must stay reachable");
    }

    let matched = client
        .query_entities(&format!("type={EXPANDED_TYPE}"), "application/json", None)
        .await
        .expect("entity query failed")
        .json_value()
        .expect("a query answer body");
    let identifiers: Vec<&str> = matched
        .as_array()
        .map(|entities| entities.iter().filter_map(|entity| entity["id"].as_str()).collect())
        .unwrap_or_default();
    for identifier in [&single, &created, &upserted, &updated, &merged] {
        assert!(
            identifiers.contains(&identifier.as_str()),
            "a type query has to find {identifier}, got {identifiers:?}"
        );
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.1.2 defines a prefix's own term before using it, so a prefix that is
/// itself declared through another prefix resolves to the IRI at the root of the
/// chain -- on the batch path exactly as on the single one.
#[tokio::test]
async fn regression_term_declaration_prefix_resolution_follows_a_nested_prefix_chain() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!({
        "base": NAMESPACE,
        "sub": "base:ontology/",
        "Place": "sub:Place",
        "address": "sub:address"
    });

    let single = scope.id("Place:chain-single");
    create_one(client, &place(&single, &context)).await;

    let batched = scope.id("Place:chain-batched");
    let response = client.batch_create(&json!([place(&batched, &context)])).await.expect("batch create failed");
    assert_eq!(response.status, StatusCode::CREATED, "batch create: {:?}", response.body);

    for identifier in [&single, &batched] {
        let entity = stored(client, identifier).await;
        assert_eq!(entity["type"], json!("http://example.org/ontology/Place"), "{identifier}: {entity}");
        assert!(entity.get("http://example.org/ontology/address").is_some(), "{identifier}: {entity}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// A term whose `@id` is already an absolute IRI names that IRI and nothing
/// else, whichever path writes it.
#[tokio::test]
async fn regression_term_declaration_prefix_resolution_keeps_an_absolute_id_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!({"Place": EXPANDED_TYPE, "address": {"@id": EXPANDED_ADDRESS}});

    let single = scope.id("Place:absolute-single");
    create_one(client, &place(&single, &context)).await;

    let batched = scope.id("Place:absolute-batched");
    let response = client.batch_create(&json!([place(&batched, &context)])).await.expect("batch create failed");
    assert_eq!(response.status, StatusCode::CREATED, "batch create: {:?}", response.body);

    for identifier in [&single, &batched] {
        let entity = stored(client, identifier).await;
        assert_eq!(entity["type"], json!(EXPANDED_TYPE), "{identifier}: {entity}");
        assert!(entity.get(EXPANDED_ADDRESS).is_some(), "{identifier}: {entity}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.1.3: with no term definition for the prefix, a `prefix:suffix`
/// declaration is already an absolute IRI and stands exactly as written. Both
/// paths keep it, so resolving a declared prefix does not invent one for an
/// undeclared prefix.
#[tokio::test]
async fn regression_term_declaration_prefix_resolution_keeps_an_undeclared_prefix_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!({"Place": "unknown:Place", "address": "unknown:address"});

    let single = scope.id("Place:unknown-single");
    create_one(client, &place(&single, &context)).await;

    let batched = scope.id("Place:unknown-batched");
    let response = client.batch_create(&json!([place(&batched, &context)])).await.expect("batch create failed");
    assert_eq!(response.status, StatusCode::CREATED, "batch create: {:?}", response.body);

    for identifier in [&single, &batched] {
        let entity = stored(client, identifier).await;
        assert_eq!(entity["type"], json!("unknown:Place"), "{identifier}: {entity}");
        assert!(entity.get("unknown:address").is_some(), "{identifier}: {entity}");
    }

    scope.finish().await.expect("environment cleanup");
}
