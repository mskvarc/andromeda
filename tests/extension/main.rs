//! Extension header integration tests using the standard Rust harness.
//!
//! Verifies that NGSI-LD extension request headers (NGSILD-Response-Format,
//! NGSILD-Dataset-Id, NGSILD-Join, NGSILD-JoinLevel, NGSILD-Scope) act as
//! fallbacks when the corresponding query parameters are absent, and that query
//! parameters take precedence when both are present simultaneously.
//!
//! Naming convention: `extension_headers_<feature>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
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

fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn reset_broker_state() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

fn unique_entity_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

async fn create_entity(http_client: &Client, body: &Value) -> StatusCode {
    http_client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create entity request failed")
        .status()
}

async fn get_entity_with_headers(http_client: &Client, entity_id: &str, query: &str, extra_headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let encoded_id = urlencoding::encode(entity_id);
    let mut request = http_client
        .get(format!("{BASE_URL}/entities/{encoded_id}{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header());
    for (name, value) in extra_headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("get entity request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

async fn query_entities_with_headers(http_client: &Client, query: &str, extra_headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut request = http_client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header());
    for (name, value) in extra_headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("query entities request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

fn building_entity(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": name},
        "@context": [CORE_CONTEXT]
    })
}

fn building_entity_with_scope(id: &str, scope: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "scope": scope,
        "@context": [CORE_CONTEXT]
    })
}

/// Vehicle entity with two multi-instance `speed` attributes, one of which carries
/// a datasetId, and a `isParked` relationship that also carries a datasetId.
fn vehicle_entity_with_two_dataset_instances(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {"type": "Property", "value": "Mercedes"},
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Person:Bob"
            },
            "datasetId": "urn:ngsi-ld:Relationship:parked12345"
        },
        "speed": [
            {
                "type": "Property",
                "value": 55,
                "source": {"type": "Property", "value": "Speedometer"}
            },
            {
                "type": "Property",
                "value": 54.5,
                "source": {"type": "Property", "value": "GPS"},
                "datasetId": "urn:ngsi-ld:Property:gpsBxyz123-speed"
            }
        ],
        "@context": [CORE_CONTEXT]
    })
}

// --- NGSILD-Response-Format ---

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_response_format_keyvalues_via_header_produces_simplified_output() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Building:ext-format-fallback");
    let status = create_entity(&http_client, &building_entity(&id, "HQ")).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity_with_headers(&http_client, &id, "", &[("NGSILD-Response-Format", "keyValues")]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"], "HQ", "NGSILD-Response-Format: keyValues must produce key-value output");
}

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_response_format_query_param_overrides_header() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Building:ext-format-precedence");
    let status = create_entity(&http_client, &building_entity(&id, "HQ")).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // options=normalized in query + NGSILD-Response-Format: keyValues in header → query param wins
    let (status, body) = get_entity_with_headers(&http_client, &id, "options=normalized", &[("NGSILD-Response-Format", "keyValues")]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(
        body["name"]["type"],
        "Property",
        "options query param must override NGSILD-Response-Format header"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_response_format_normalized_without_header_returns_full_attribute_object() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Building:ext-format-normalized");
    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Test Building"},
        "height": {"type": "Property", "value": 42},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // No format header or query param → normalized default
    let (status, body) = get_entity_with_headers(&http_client, &id, "", &[]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body["name"].is_object(), "default response must return full attribute object");
    pretty_assertions::assert_eq!(body["name"]["type"], "Property");

    // With NGSILD-Response-Format: keyValues → simplified values
    let (status, kv_body) = get_entity_with_headers(&http_client, &id, "", &[("NGSILD-Response-Format", "keyValues")]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(kv_body["name"], "Test Building");
    pretty_assertions::assert_eq!(kv_body["height"], 42);
}

// --- NGSILD-Dataset-Id ---

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_dataset_id_via_header_matches_query_param_result() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:ext-dataset-fallback");
    let status = create_entity(&http_client, &vehicle_entity_with_two_dataset_instances(&id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let dataset = "urn:ngsi-ld:Property:gpsBxyz123-speed";
    let encoded_dataset = urlencoding::encode(dataset);

    let (status, header_body) = get_entity_with_headers(&http_client, &id, "", &[("NGSILD-Dataset-Id", dataset)]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let (status, query_body) = get_entity_with_headers(&http_client, &id, &format!("datasetId={encoded_dataset}"), &[]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        header_body,
        query_body,
        "NGSILD-Dataset-Id header must produce the same result as datasetId query param"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_dataset_id_query_param_overrides_header() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:ext-dataset-precedence");
    let status = create_entity(&http_client, &vehicle_entity_with_two_dataset_instances(&id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query_dataset = "urn:ngsi-ld:Relationship:parked12345";
    let header_dataset = "urn:ngsi-ld:Property:gpsBxyz123-speed";
    let encoded_query_dataset = urlencoding::encode(query_dataset);

    // Query param only — establishes baseline
    let (status, query_only_body) = get_entity_with_headers(&http_client, &id, &format!("datasetId={encoded_query_dataset}"), &[]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // Both query param and a different dataset in header — query param must win
    let (status, both_body) = get_entity_with_headers(
        &http_client,
        &id,
        &format!("datasetId={encoded_query_dataset}"),
        &[("NGSILD-Dataset-Id", header_dataset)],
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    pretty_assertions::assert_eq!(
        query_only_body,
        both_body,
        "datasetId query param must take precedence over NGSILD-Dataset-Id header"
    );
}

// --- NGSILD-Join / NGSILD-JoinLevel ---

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_join_via_header_includes_linked_entities_in_flat_response() {
    let http_client = client();
    reset_broker_state().await;

    let linked_id = unique_entity_id("urn:ngsi-ld:Entity:ext-join-linked");
    let linked = json!({
        "id": linked_id,
        "type": "Building",
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &linked).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let root_id = unique_entity_id("urn:ngsi-ld:Entity:ext-join-root");
    let root = json!({
        "id": root_id,
        "type": "Building",
        "linkedTo": {"type": "Relationship", "object": linked_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &root).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let encoded_root_id = urlencoding::encode(&root_id);
    let (status, body) = query_entities_with_headers(
        &http_client,
        &format!("id={encoded_root_id}&type=Building"),
        &[("NGSILD-Join", "flat"), ("NGSILD-JoinLevel", "1")],
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("flat join query must return an array");
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids.contains(&root_id.as_str()), "flat join result must contain root entity");
    assert!(ids.contains(&linked_id.as_str()), "flat join result must contain the linked entity");
}

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_join_level_header_limits_traversal_depth() {
    let http_client = client();
    reset_broker_state().await;

    // Build a 3-entity chain: root → middle → leaf
    let leaf_id = unique_entity_id("urn:ngsi-ld:Entity:ext-joinlevel-leaf");
    let leaf = json!({
        "id": leaf_id,
        "type": "Building",
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &leaf).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let middle_id = unique_entity_id("urn:ngsi-ld:Entity:ext-joinlevel-middle");
    let middle = json!({
        "id": middle_id,
        "type": "Building",
        "linksTo": {"type": "Relationship", "object": leaf_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &middle).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let root_id = unique_entity_id("urn:ngsi-ld:Entity:ext-joinlevel-root");
    let root = json!({
        "id": root_id,
        "type": "Building",
        "linksTo": {"type": "Relationship", "object": middle_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &root).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let encoded_root_id = urlencoding::encode(&root_id);

    // NGSILD-JoinLevel: 1 → root + middle only, leaf excluded
    let (status, body_level_1) = query_entities_with_headers(
        &http_client,
        &format!("id={encoded_root_id}&type=Building"),
        &[("NGSILD-Join", "flat"), ("NGSILD-JoinLevel", "1")],
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr_1 = body_level_1.as_array().expect("level-1 join must return array");
    let ids_1: Vec<&str> = arr_1.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids_1.contains(&root_id.as_str()));
    assert!(ids_1.contains(&middle_id.as_str()));
    assert!(!ids_1.contains(&leaf_id.as_str()), "NGSILD-JoinLevel: 1 must not traverse beyond one hop");

    // NGSILD-JoinLevel: 2 → root + middle + leaf all present
    let (status, body_level_2) = query_entities_with_headers(
        &http_client,
        &format!("id={encoded_root_id}&type=Building"),
        &[("NGSILD-Join", "flat"), ("NGSILD-JoinLevel", "2")],
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let arr_2 = body_level_2.as_array().expect("level-2 join must return array");
    let ids_2: Vec<&str> = arr_2.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids_2.contains(&root_id.as_str()));
    assert!(ids_2.contains(&middle_id.as_str()));
    assert!(ids_2.contains(&leaf_id.as_str()), "NGSILD-JoinLevel: 2 must traverse two hops and include leaf");
}

// --- NGSILD-Scope ---

#[serial_test::serial]
#[tokio::test]
async fn extension_headers_scope_via_header_filters_entities_by_scope_path() {
    let http_client = client();
    reset_broker_state().await;

    let id_a = unique_entity_id("urn:ngsi-ld:Building:ext-scope-a");
    let id_b = unique_entity_id("urn:ngsi-ld:Building:ext-scope-b");

    let status = create_entity(&http_client, &building_entity_with_scope(&id_a, "/Company/AreaA")).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let status = create_entity(&http_client, &building_entity_with_scope(&id_b, "/Company/AreaB")).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = query_entities_with_headers(&http_client, "type=Building", &[("NGSILD-Scope", "/Company/AreaA")]).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result must be an array");
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids.contains(&id_a.as_str()), "entity in /Company/AreaA must appear in scoped results");
    assert!(
        !ids.contains(&id_b.as_str()),
        "entity in /Company/AreaB must be excluded from /Company/AreaA scope"
    );
}
