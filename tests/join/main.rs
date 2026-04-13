//! Join traversal and `containedBy` integration tests (ETSI GS CIM 009 clause 6.4.3.2, 6.5.3.1).
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Covers:
//! - `join=flat` includes directly-linked entities in results.
//! - `joinLevel` limits the traversal depth.
//! - `containedBy` excludes specified entity IDs from linked-entity resolution.
//! - Cycle detection prevents infinite loops during join traversal.
//! - Single-entity GET with `join=flat` returns an array.
//! - Single-entity GET respects `containedBy` exclusion.
//! - `joinLevel=0` returns only the root entity with no linked entities.
//! - `containedBy` with multiple excluded IDs excludes all of them.
//!
//! Naming convention: `join_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn link_header() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

/// POST /entities. Returns (status, location).
async fn create_entity(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// GET /entities with query string.
async fn query_entities(client: &Client, query: &str) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /entities/{id} with query string.
async fn get_entity(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities/{id}{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("get_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

fn ids_from_array(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be array")
        .iter()
        .filter_map(|e| e["id"].as_str().map(str::to_owned))
        .collect()
}

// ---------------------------------------------------------------------------
// join=flat -- includes linked entities
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn join_flat_includes_directly_linked_entities() {
    // A --linkedTo--> B.  Query A with join=flat,joinLevel=1 must return both.
    clean_database().await;
    let c = client();
    let id_b = unique_id("urn:ngsi-ld:Building:jf-B");
    let id_a = unique_id("urn:ngsi-ld:Building:jf-A");

    let b = json!({
        "id": id_b, "type": "Building",
        "name": {"type": "Property", "value": "B"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &b).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let a = json!({
        "id": id_a, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_b},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &a).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={id_a}&type=Building&join=flat&joinLevel=1");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a), "root entity must be in results");
    assert!(ids.contains(&id_b), "linked entity must be included with join=flat");
}

#[tokio::test]
#[serial]
async fn join_flat_join_level_limits_traversal_depth() {
    // A --linkedTo--> B --linkedTo--> C.
    // join=flat&joinLevel=1 must return A and B only, not C.
    clean_database().await;
    let c = client();
    let id_c = unique_id("urn:ngsi-ld:Building:jl-C");
    let id_b = unique_id("urn:ngsi-ld:Building:jl-B");
    let id_a = unique_id("urn:ngsi-ld:Building:jl-A");

    for (id, linked) in [(&id_c, None), (&id_b, Some(&id_c)), (&id_a, Some(&id_b))] {
        let mut body = json!({
            "id": id, "type": "Building",
            "@context": [TEST_SUITE_CONTEXT]
        });
        if let Some(target) = linked {
            body["linkedTo"] = json!({"type": "Relationship", "object": target});
        }
        let (status, _) = create_entity(&c, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let query = format!("id={id_a}&type=Building&join=flat&joinLevel=1");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a), "root entity must be present");
    assert!(ids.contains(&id_b), "level-1 entity must be present");
    assert!(!ids.contains(&id_c), "level-2 entity must be excluded when joinLevel=1");
}

#[tokio::test]
#[serial]
async fn join_flat_join_level_two_includes_two_hops() {
    // A --linkedTo--> B --linkedTo--> C.
    // join=flat&joinLevel=2 must return A, B, and C.
    clean_database().await;
    let c = client();
    let id_c = unique_id("urn:ngsi-ld:Building:jl2-C");
    let id_b = unique_id("urn:ngsi-ld:Building:jl2-B");
    let id_a = unique_id("urn:ngsi-ld:Building:jl2-A");

    for (id, linked) in [(&id_c, None), (&id_b, Some(&id_c)), (&id_a, Some(&id_b))] {
        let mut body = json!({
            "id": id, "type": "Building",
            "@context": [TEST_SUITE_CONTEXT]
        });
        if let Some(target) = linked {
            body["linkedTo"] = json!({"type": "Relationship", "object": target});
        }
        let (status, _) = create_entity(&c, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let query = format!("id={id_a}&type=Building&join=flat&joinLevel=2");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a), "root entity must be present");
    assert!(ids.contains(&id_b), "level-1 entity must be present");
    assert!(ids.contains(&id_c), "level-2 entity must be present when joinLevel=2");
}

// ---------------------------------------------------------------------------
// containedBy -- excludes specified IDs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn join_contained_by_excludes_specified_entity_from_join_results() {
    // A --linkedTo--> B --linkedTo--> C.
    // containedBy=B excludes B; C is not reached because B was not followed.
    clean_database().await;
    let c = client();
    let id_c = unique_id("urn:ngsi-ld:Building:cb-C");
    let id_b = unique_id("urn:ngsi-ld:Building:cb-B");
    let id_a = unique_id("urn:ngsi-ld:Building:cb-A");

    let entity_c = json!({
        "id": id_c, "type": "Building",
        "name": {"type": "Property", "value": "C"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity_c).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let entity_b = json!({
        "id": id_b, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_c},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity_b).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let entity_a = json!({
        "id": id_a, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_b},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity_a).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={id_a}&type=Building&join=flat&joinLevel=2&containedBy={id_b}");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a), "root entity must be present");
    assert!(!ids.contains(&id_b), "B must be excluded by containedBy");
}

#[tokio::test]
#[serial]
async fn join_contained_by_multiple_ids_excludes_all_listed_entities() {
    // A --linkedTo--> B, A --alsoLinkedTo--> C.
    // containedBy=B,C must exclude both.
    clean_database().await;
    let c = client();
    let id_b = unique_id("urn:ngsi-ld:Building:cbm-B");
    let id_c = unique_id("urn:ngsi-ld:Building:cbm-C");
    let id_a = unique_id("urn:ngsi-ld:Building:cbm-A");

    for id in [&id_b, &id_c] {
        let body = json!({
            "id": id, "type": "Building",
            "@context": [TEST_SUITE_CONTEXT]
        });
        let (status, _) = create_entity(&c, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let entity_a = json!({
        "id": id_a, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_b},
        "alsoLinkedTo": {"type": "Relationship", "object": id_c},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity_a).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={id_a}&type=Building&join=flat&joinLevel=1&containedBy={id_b},{id_c}");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a), "root entity must be present");
    assert!(!ids.contains(&id_b), "B must be excluded by containedBy");
    assert!(!ids.contains(&id_c), "C must be excluded by containedBy");
}

// ---------------------------------------------------------------------------
// Cycle detection
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn join_cycle_detection_does_not_infinite_loop() {
    // X --linkedTo--> Y --linkedTo--> X (cycle).
    // join traversal must terminate and return both entities.
    clean_database().await;
    let c = client();
    let id_x = unique_id("urn:ngsi-ld:Building:cyc-X");
    let id_y = unique_id("urn:ngsi-ld:Building:cyc-Y");

    let x = json!({
        "id": id_x, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_y},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &x).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let y = json!({
        "id": id_y, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_x},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &y).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={id_x}&type=Building&join=flat&joinLevel=5");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.len() >= 2, "both entities must be returned despite cycle");
}

// ---------------------------------------------------------------------------
// Single-entity GET with join
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn join_single_entity_get_flat_join_returns_array_with_linked_entity() {
    // GET /entities/{id}?join=flat must return an array [root, ...linked].
    clean_database().await;
    let c = client();
    let id_target = unique_id("urn:ngsi-ld:Building:sg-target");
    let id_main = unique_id("urn:ngsi-ld:Building:sg-main");

    let target = json!({
        "id": id_target, "type": "Building",
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &target).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let main_e = json!({
        "id": id_main, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_target},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &main_e).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id_main, "join=flat&joinLevel=1").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("flat join on single GET must return array");
    let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&id_main.as_str()), "root entity must be present");
    assert!(ids.contains(&id_target.as_str()), "linked entity must be present");
}

#[tokio::test]
#[serial]
async fn join_single_entity_get_with_contained_by_excludes_linked_entity() {
    clean_database().await;
    let c = client();
    let id_target = unique_id("urn:ngsi-ld:Building:sgcb-target");
    let id_main = unique_id("urn:ngsi-ld:Building:sgcb-main");

    let target = json!({
        "id": id_target, "type": "Building",
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &target).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let main_e = json!({
        "id": id_main, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_target},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &main_e).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("join=flat&joinLevel=1&containedBy={id_target}");
    let (status, body) = get_entity(&c, &id_main, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("flat join on single GET must return array");
    let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&id_main.as_str()), "root entity must be present");
    assert!(!ids.contains(&id_target.as_str()), "target entity must be excluded by containedBy");
}

#[tokio::test]
#[serial]
async fn join_join_level_zero_returns_only_root_entity_without_linked() {
    // joinLevel=0 means no linked-entity resolution at all.
    clean_database().await;
    let c = client();
    let id_b = unique_id("urn:ngsi-ld:Building:jl0-B");
    let id_a = unique_id("urn:ngsi-ld:Building:jl0-A");

    let b = json!({ "id": id_b, "type": "Building", "@context": [TEST_SUITE_CONTEXT] });
    let (status, _) = create_entity(&c, &b).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let a = json!({
        "id": id_a, "type": "Building",
        "linkedTo": {"type": "Relationship", "object": id_b},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &a).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = format!("id={id_a}&type=Building&join=flat&joinLevel=0");
    let (status, body) = query_entities(&c, &query).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a), "root entity must be present");
    assert!(!ids.contains(&id_b), "linked entity must be absent when joinLevel=0");
}
