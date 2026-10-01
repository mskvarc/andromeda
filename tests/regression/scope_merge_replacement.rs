//! Merging an entity `scope` replaces it as a whole.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.17.4: "If scope is included in the
//! Fragment and overwrite is allowed, the scope of the target Entity will become
//! the one included in the Fragment." Merge Entity takes no `noOverwrite` option
//! (clause 5.6.17.3), and Batch Entity Merge runs the same behaviour for each
//! entity (clause 5.6.20.4). Clause 4.18 makes `scope` one non-reified member
//! with one value, so the Scopes it lists are not attribute instances told apart
//! by `datasetId`.
//!
//! The broker merged them as if they were: every fragment Scope matched the
//! first stored one, so each overwrote position 0 in turn and the stored tail
//! survived. `["/A/B","/C/D"]` merged onto `["/X/A","/Y/A","/Z/A"]` read back as
//! `["/C/D","/Y/A","/Z/A"]`, and `scopeQ` answered from that corrupted value.
//! A merge onto a single Scope or onto no Scope looked right only because one
//! overwritten position was the whole value.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{entity::results_count, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The NGSI-LD Null, which clause 4.18 spells a deleted scope with.
const NGSI_LD_NULL: &str = "urn:ngsi-ld:null";

/// One `Building` carrying a `temperature` and, when given, a `scope`.
fn building(id: &str, scope: Option<&Value>) -> Value {
    let mut entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 21.5},
        "@context": [TEST_SUITE_CONTEXT]
    });
    if let Some(scope) = scope {
        entity["scope"] = scope.clone(); // clone: the caller keeps its scope for later assertions
    }
    entity
}

/// Create one `Building` and assert clause 6.4.3.1 answered with 201.
async fn create_building(client: &TestClient, id: &str, scope: Option<&Value>) {
    let response = client
        .post_entity(&building(id, scope))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {id}: {:?}", response.body);
}

/// Merge a fragment into one entity and assert clause 6.5.3.4 answered with 204.
async fn merge(client: &TestClient, id: &str, fragment: &Value) {
    let response = client.merge_entity(id, fragment, None).await.expect("merge entity request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "merge {id}: {:?}", response.body);
}

/// Merge a fragment naming nothing but `scope` into one entity.
async fn merge_scope(client: &TestClient, id: &str, scope: &Value) {
    merge(client, id, &json!({"scope": scope, "@context": [TEST_SUITE_CONTEXT]})).await;
}

/// The `scope` member of the stored entity, absent when it carries none.
///
/// One Scope compacts to a string and several to an array in stored order.
async fn stored_scope(client: &TestClient, id: &str) -> Option<Value> {
    let response = client
        .get_entity(id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    let body = response.json_value().expect("the retrieved entity is JSON");
    body.get("scope").cloned()
}

/// How many entities with this id the clause 4.19 `scopeQ` pattern selects.
async fn scope_count(client: &TestClient, id: &str, pattern: &str) -> Option<usize> {
    let query = format!(
        "type=Building&id={}&scopeQ={}&count=true&limit=0",
        urlencoding::encode(id),
        urlencoding::encode(pattern)
    );
    let response = client
        .query_entities(&query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query entities request");
    assert_eq!(response.status, StatusCode::OK, "scopeQ={pattern}: {:?}", response.body);
    results_count(&response.headers)
}

/// Assert the `scopeQ` count of every pattern for one entity.
async fn assert_scope_counts(client: &TestClient, id: &str, expected: &[(&str, usize)]) {
    for (pattern, count) in expected {
        assert_eq!(scope_count(client, id, pattern).await, Some(*count), "scopeQ={pattern} on {id}");
    }
}

/// Clause 5.6.17.4: the merged scope is the fragment's. Before the fix the
/// entity read back `["/C/D","/Y/A","/Z/A"]`, `/A/B` matched nothing and `/Y/A`
/// still matched.
#[tokio::test]
async fn regression_scope_merge_entity_replaces_an_array_with_an_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:scope-merge-array");
    create_building(client, &id, Some(&json!(["/X/A", "/Y/A", "/Z/A"]))).await;

    merge_scope(client, &id, &json!(["/A/B", "/C/D"])).await;

    assert_eq!(stored_scope(client, &id).await, Some(json!(["/A/B", "/C/D"])));
    assert_scope_counts(client, &id, &[("/A/B", 1), ("/C/D", 1), ("/X/A", 0), ("/Y/A", 0), ("/Z/A", 0)]).await;

    scope.finish().await.expect("cleanup failed");
}

/// A single Scope replaces a stored array whole. Before the fix it overwrote
/// only the first stored Scope: `["/G/H","/Y/A","/Z/A"]`.
#[tokio::test]
async fn regression_scope_merge_entity_replaces_an_array_with_a_string() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:scope-merge-string");
    create_building(client, &id, Some(&json!(["/X/A", "/Y/A", "/Z/A"]))).await;

    merge_scope(client, &id, &json!("/G/H")).await;

    assert_eq!(stored_scope(client, &id).await, Some(json!("/G/H")));
    assert_scope_counts(client, &id, &[("/G/H", 1), ("/Y/A", 0)]).await;

    scope.finish().await.expect("cleanup failed");
}

/// A fragment whose first Scope equals the stored first Scope keeps it, in the
/// fragment's order. Before the fix both fragment Scopes landed on position 0
/// one after the other, losing `/X/A` and keeping `/Y/A`: `["/Q/R","/Y/A"]`.
#[tokio::test]
async fn regression_scope_merge_entity_keeps_the_fragment_order_for_an_overlapping_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:scope-merge-overlap");
    create_building(client, &id, Some(&json!(["/X/A", "/Y/A"]))).await;

    merge_scope(client, &id, &json!(["/X/A", "/Q/R"])).await;

    assert_eq!(stored_scope(client, &id).await, Some(json!(["/X/A", "/Q/R"])));
    assert_scope_counts(client, &id, &[("/X/A", 1), ("/Q/R", 1), ("/Y/A", 0)]).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12: members of the Fragment that do not appear within the target
/// "are added to the target".
#[tokio::test]
async fn regression_scope_merge_entity_sets_a_scope_on_an_entity_without_one() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:scope-merge-absent");
    create_building(client, &id, None).await;

    merge_scope(client, &id, &json!(["/A/B", "/C/D"])).await;

    assert_eq!(stored_scope(client, &id).await, Some(json!(["/A/B", "/C/D"])));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 removes the member a fragment sets to the NGSI-LD Null, which
/// clause 4.18 reserves for a deleted scope, whatever number of Scopes it held.
#[tokio::test]
async fn regression_scope_merge_entity_deletes_the_scope_with_the_ngsi_ld_null() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:scope-merge-null");
    create_building(client, &id, Some(&json!(["/X/A", "/Y/A"]))).await;

    merge_scope(client, &id, &json!(NGSI_LD_NULL)).await;

    assert_eq!(stored_scope(client, &id).await, None);
    assert_scope_counts(client, &id, &[("/X/A", 0)]).await;

    scope.finish().await.expect("cleanup failed");
}

/// A merge whose fragment names no `scope` leaves the stored one as it was.
#[tokio::test]
async fn regression_scope_merge_entity_without_scope_leaves_the_scope_untouched() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:scope-merge-untouched");
    create_building(client, &id, Some(&json!(["/X/A", "/Y/A"]))).await;

    merge(
        client,
        &id,
        &json!({"temperature": {"type": "Property", "value": 30}, "@context": [TEST_SUITE_CONTEXT]}),
    )
    .await;

    assert_eq!(stored_scope(client, &id).await, Some(json!(["/X/A", "/Y/A"])));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.20.4 runs the clause 5.6.17 behaviour for each entity of a
/// Batch Entity Merge, so every shape of scope lands there exactly as it does
/// through Merge Entity.
#[tokio::test]
async fn regression_scope_batch_merge_replaces_every_shape_of_scope() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let cases = [
        (
            scope.id("Building:scope-batch-array"),
            Some(json!(["/X/A", "/Y/A", "/Z/A"])),
            json!(["/A/B", "/C/D"]),
            Some(json!(["/A/B", "/C/D"])),
        ),
        (
            scope.id("Building:scope-batch-string"),
            Some(json!(["/X/A", "/Y/A", "/Z/A"])),
            json!("/G/H"),
            Some(json!("/G/H")),
        ),
        (
            scope.id("Building:scope-batch-absent"),
            None,
            json!(["/A/B", "/C/D"]),
            Some(json!(["/A/B", "/C/D"])),
        ),
        (scope.id("Building:scope-batch-null"), Some(json!(["/X/A", "/Y/A"])), json!(NGSI_LD_NULL), None),
    ];
    for (id, created, _, _) in &cases {
        create_building(client, id, created.as_ref()).await;
    }

    let body = Value::Array(
        cases
            .iter()
            .map(|(id, _, merged, _)| json!({"id": id, "type": "Building", "scope": merged, "@context": [TEST_SUITE_CONTEXT]}))
            .collect(),
    );
    let response = client.batch_merge(&body).await.expect("batch merge request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "batch merge: {:?}", response.body);

    for (id, _, _, expected) in &cases {
        assert_eq!(stored_scope(client, id).await, *expected, "scope of {id}");
    }

    scope.finish().await.expect("cleanup failed");
}
