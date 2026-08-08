//! Join traversal and `containedBy` integration tests (NGSI-LD 6.4.3.2, 6.5.3.1).
//!
//! Naming convention: `join_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn create(client: &NgsiLdClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn query(client: &NgsiLdClient, query_str: &str) -> Value {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.body
}

async fn get_one(client: &NgsiLdClient, id: &str, query_str: &str) -> Value {
    let response = client
        .get_entity(id, query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    response.body
}

fn ids_from_array(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

fn minimal_building(id: &str) -> Value {
    json!({"id": id, "type": "Building", "@context": [TEST_SUITE_CONTEXT]})
}

fn linked_building(id: &str, target: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "linkedTo": {"type": "Relationship", "object": target},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

#[tokio::test]
async fn join_flat_includes_directly_linked_entities() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_b = scope.id("Building:jf-B");
    let id_a = scope.id("Building:jf-A");
    create(&client, &minimal_building(&id_b)).await;
    create(&client, &linked_building(&id_a, &id_b)).await;

    let body = query(&client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_flat_join_level_limits_traversal_depth() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_c = scope.id("Building:jl-C");
    let id_b = scope.id("Building:jl-B");
    let id_a = scope.id("Building:jl-A");

    create(&client, &minimal_building(&id_c)).await;
    create(&client, &linked_building(&id_b, &id_c)).await;
    create(&client, &linked_building(&id_a, &id_b)).await;

    let body = query(&client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    assert!(!ids.contains(&id_c), "joinLevel=1 must exclude level-2 entity");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_flat_join_level_two_includes_two_hops() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_c = scope.id("Building:jl2-C");
    let id_b = scope.id("Building:jl2-B");
    let id_a = scope.id("Building:jl2-A");

    create(&client, &minimal_building(&id_c)).await;
    create(&client, &linked_building(&id_b, &id_c)).await;
    create(&client, &linked_building(&id_a, &id_b)).await;

    let body = query(&client, &format!("id={id_a}&type=Building&join=flat&joinLevel=2")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    assert!(ids.contains(&id_c));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_contained_by_excludes_specified_entity_from_join_results() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_c = scope.id("Building:cb-C");
    let id_b = scope.id("Building:cb-B");
    let id_a = scope.id("Building:cb-A");

    create(&client, &minimal_building(&id_c)).await;
    create(&client, &linked_building(&id_b, &id_c)).await;
    create(&client, &linked_building(&id_a, &id_b)).await;

    let body = query(&client, &format!("id={id_a}&type=Building&join=flat&joinLevel=2&containedBy={id_b}")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(!ids.contains(&id_b), "B must be excluded by containedBy");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_contained_by_multiple_ids_excludes_all_listed_entities() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_b = scope.id("Building:cbm-B");
    let id_c = scope.id("Building:cbm-C");
    let id_a = scope.id("Building:cbm-A");

    for id in [&id_b, &id_c] {
        create(&client, &minimal_building(id)).await;
    }
    create(
        &client,
        &json!({
            "id": id_a,
            "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_b},
            "alsoLinkedTo": {"type": "Relationship", "object": id_c},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query(&client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1&containedBy={id_b},{id_c}")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(!ids.contains(&id_b));
    assert!(!ids.contains(&id_c));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_cycle_detection_does_not_infinite_loop() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_x = scope.id("Building:cyc-X");
    let id_y = scope.id("Building:cyc-Y");

    create(&client, &linked_building(&id_x, &id_y)).await;
    create(&client, &linked_building(&id_y, &id_x)).await;

    let body = query(&client, &format!("id={id_x}&type=Building&join=flat&joinLevel=5")).await;
    let ids = ids_from_array(&body);
    assert!(ids.len() >= 2, "both entities must be returned despite cycle");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_single_entity_get_flat_join_returns_array_with_linked_entity() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_target = scope.id("Building:sg-target");
    let id_main = scope.id("Building:sg-main");
    create(&client, &minimal_building(&id_target)).await;
    create(&client, &linked_building(&id_main, &id_target)).await;

    let body = get_one(&client, &id_main, "join=flat&joinLevel=1").await;
    let arr = body.as_array().expect("flat join on single GET must return array");
    let ids: Vec<&str> = arr.iter().filter_map(|entity| entity["id"].as_str()).collect();
    assert!(ids.contains(&id_main.as_str()));
    assert!(ids.contains(&id_target.as_str()));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_single_entity_get_with_contained_by_excludes_linked_entity() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let id_target = scope.id("Building:sgcb-target");
    let id_main = scope.id("Building:sgcb-main");
    create(&client, &minimal_building(&id_target)).await;
    create(&client, &linked_building(&id_main, &id_target)).await;

    let body = get_one(&client, &id_main, &format!("join=flat&joinLevel=1&containedBy={id_target}")).await;
    let arr = body.as_array().expect("flat join on single GET must return array");
    let ids: Vec<&str> = arr.iter().filter_map(|entity| entity["id"].as_str()).collect();
    assert!(ids.contains(&id_main.as_str()));
    assert!(!ids.contains(&id_target.as_str()));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_join_level_zero_returns_bad_request_data() {
    let scope = TestScope::new("join");
    let client = NgsiLdClient::new(&scope).await;
    let response = client
        .query_entities("type=Building&join=flat&joinLevel=0", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert!(
        response.body["type"]
            .as_str()
            .is_some_and(|problem_type| problem_type.ends_with("BadRequestData"))
    );
    scope.cleanup().await.expect("cleanup failed");
}
