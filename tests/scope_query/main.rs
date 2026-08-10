//! Scope query integration tests (NGSI-LD 4.18 scope membership, 4.19 scopeQ).
//!
//! Covers the scopeQ pattern language operators — exact path, ancestor
//! glob (`/a/b/*`), and disjunction (`+`, `,`) — against the `/entities`
//! query endpoint.
//!
//! Naming convention: `scope_query_<operator>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn query(client: &TestClient, query_str: &str) -> Value {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

fn sanitized_root(scope: &TestEnvironment) -> String {
    format!("T{}", scope.id_prefix().chars().filter(char::is_ascii_alphanumeric).collect::<String>())
}

fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

async fn create_scoped_building(client: &TestClient, id: &str, scope_path: &str) {
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "scope": scope_path,
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
}

#[tokio::test]
async fn scope_query_exact_path_matches_only_that_entity() {
    let scope = TestEnvironment::from_suite("scope_query").await.expect("isolated test environment");
    let client = scope.client();
    let root = sanitized_root(&scope);

    let id_a = scope.id("Building:exact-a");
    let id_b = scope.id("Building:exact-b");
    create_scoped_building(client, &id_a, &format!("/{root}/AreaA")).await;
    create_scoped_building(client, &id_b, &format!("/{root}/AreaB")).await;

    let filter = id_filter(&[&id_a, &id_b]);
    let body = query(
        client,
        &format!("type=Building&id={filter}&scopeQ={}", urlencoding::encode(&format!("/{root}/AreaA"))),
    )
    .await;
    let ids = ids_of(&body);
    assert!(ids.contains(&id_a), "exact /{root}/AreaA must match id_a");
    assert!(!ids.contains(&id_b), "/{root}/AreaB must not match the exact /{root}/AreaA path");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn scope_query_descendant_glob_matches_nested_scopes() {
    let scope = TestEnvironment::from_suite("scope_query").await.expect("isolated test environment");
    let client = scope.client();
    let root = sanitized_root(&scope);

    let id_a = scope.id("Building:desc-a");
    let id_b = scope.id("Building:desc-b");
    let id_outside = scope.id("Building:desc-outside");
    create_scoped_building(client, &id_a, &format!("/{root}/AreaA/Zone1")).await;
    create_scoped_building(client, &id_b, &format!("/{root}/AreaA/Zone2")).await;
    create_scoped_building(client, &id_outside, &format!("/{root}/AreaB/Zone1")).await;

    let filter = id_filter(&[&id_a, &id_b, &id_outside]);
    let body = query(
        client,
        &format!("type=Building&id={filter}&scopeQ={}", urlencoding::encode(&format!("/{root}/AreaA/#"))),
    )
    .await;
    let ids = ids_of(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    assert!(!ids.contains(&id_outside), "descendant glob must not cross sibling branches");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn scope_query_disjunction_matches_any_listed_path() {
    let scope = TestEnvironment::from_suite("scope_query").await.expect("isolated test environment");
    let client = scope.client();
    let root = sanitized_root(&scope);

    let id_a = scope.id("Building:disj-a");
    let id_b = scope.id("Building:disj-b");
    let id_c = scope.id("Building:disj-c");
    create_scoped_building(client, &id_a, &format!("/{root}/AreaA")).await;
    create_scoped_building(client, &id_b, &format!("/{root}/AreaB")).await;
    create_scoped_building(client, &id_c, &format!("/{root}/AreaC")).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let scope_q = format!("/{root}/AreaA,/{root}/AreaB");
    let body = query(client, &format!("type=Building&id={filter}&scopeQ={}", urlencoding::encode(&scope_q))).await;
    let ids = ids_of(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    assert!(!ids.contains(&id_c), "AreaC must be excluded by the two-path disjunction");
    scope.finish().await.expect("cleanup failed");
}
