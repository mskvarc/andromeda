//! Extension header integration tests.
//!
//! Verifies that NGSI-LD extension request headers (NGSILD-Response-Format,
//! NGSILD-Dataset-Id, NGSILD-Join, NGSILD-JoinLevel, NGSILD-Scope) act as
//! fallbacks when the corresponding query parameters are absent, and that query
//! parameters take precedence when both are present simultaneously.
//!
//! Naming convention: `extension_headers_<feature>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn get_entity_with_headers(client: &TestClient, entity_id: &str, query: &str, extra_headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let path = format!("/ngsi-ld/v1/entities/{}{sep}{query}", urlencoding::encode(entity_id));
    let mut request = client
        .raw(reqwest::Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("core context Link must be valid"));
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }
    let response = request.send().await.expect("get entity request failed");
    (response.status, response.json_value().expect("response body must be valid JSON"))
}

async fn query_entities_with_headers(client: &TestClient, query: &str, extra_headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let path = format!("/ngsi-ld/v1/entities{sep}{query}");
    let mut request = client
        .raw(reqwest::Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("core context Link must be valid"));
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }
    let response = request.send().await.expect("query entities request failed");
    (response.status, response.json_value().expect("response body must be valid JSON"))
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

fn vehicle_entity_with_two_dataset_instances(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": {"type": "Property", "value": "Mercedes"},
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:Bob"},
            "datasetId": "urn:ngsi-ld:Relationship:parked12345"
        },
        "speed": [
            {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}},
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

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

#[tokio::test]
async fn extension_headers_response_format_keyvalues_via_header_produces_simplified_output() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:format-fallback");
    create(client, &building_entity(&id, "HQ")).await;

    let (status, body) = get_entity_with_headers(client, &id, "", &[("NGSILD-Response-Format", "keyValues")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "HQ", "NGSILD-Response-Format: keyValues must produce key-value output");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_response_format_query_param_overrides_header() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:format-precedence");
    create(client, &building_entity(&id, "HQ")).await;

    let (status, body) = get_entity_with_headers(client, &id, "options=normalized", &[("NGSILD-Response-Format", "keyValues")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["name"]["type"], "Property",
        "options query param must override NGSILD-Response-Format header"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_response_format_normalized_without_header_returns_full_attribute_object() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:format-normalized");
    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Test Building"},
        "height": {"type": "Property", "value": 42},
        "@context": [CORE_CONTEXT]
    });
    create(client, &entity).await;

    let (status, body) = get_entity_with_headers(client, &id, "", &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["name"].is_object(), "default response must return full attribute object");
    assert_eq!(body["name"]["type"], "Property");

    let (status, kv_body) = get_entity_with_headers(client, &id, "", &[("NGSILD-Response-Format", "keyValues")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(kv_body["name"], "Test Building");
    assert_eq!(kv_body["height"], 42);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_dataset_id_via_header_matches_query_param_result() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dataset-fallback");
    create(client, &vehicle_entity_with_two_dataset_instances(&id)).await;

    let dataset = "urn:ngsi-ld:Property:gpsBxyz123-speed";
    let encoded = urlencoding::encode(dataset);

    let (status, header_body) = get_entity_with_headers(client, &id, "", &[("NGSILD-Dataset-Id", dataset)]).await;
    assert_eq!(status, StatusCode::OK);

    let (status, query_body) = get_entity_with_headers(client, &id, &format!("datasetId={encoded}"), &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_body, query_body,
        "NGSILD-Dataset-Id header must produce the same result as datasetId query param"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_dataset_id_query_param_overrides_header() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dataset-precedence");
    create(client, &vehicle_entity_with_two_dataset_instances(&id)).await;

    let query_dataset = "urn:ngsi-ld:Relationship:parked12345";
    let header_dataset = "urn:ngsi-ld:Property:gpsBxyz123-speed";
    let encoded = urlencoding::encode(query_dataset);

    let (status, query_only) = get_entity_with_headers(client, &id, &format!("datasetId={encoded}"), &[]).await;
    assert_eq!(status, StatusCode::OK);

    let (status, both) = get_entity_with_headers(client, &id, &format!("datasetId={encoded}"), &[("NGSILD-Dataset-Id", header_dataset)]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(query_only, both, "datasetId query param must take precedence over NGSILD-Dataset-Id header");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_join_via_header_includes_linked_entities_in_flat_response() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();

    let linked_id = scope.id("Entity:join-linked");
    let linked = json!({"id": linked_id, "type": "Building", "@context": [CORE_CONTEXT]});
    create(client, &linked).await;

    let root_id = scope.id("Entity:join-root");
    let root = json!({
        "id": root_id,
        "type": "Building",
        "linkedTo": {"type": "Relationship", "object": linked_id},
        "@context": [CORE_CONTEXT]
    });
    create(client, &root).await;

    let encoded_root = urlencoding::encode(&root_id);
    let (status, body) = query_entities_with_headers(
        client,
        &format!("id={encoded_root}&type=Building"),
        &[("NGSILD-Join", "flat"), ("NGSILD-JoinLevel", "1")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("flat join query must return an array");
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids.contains(&root_id.as_str()), "flat join result must contain root entity");
    assert!(ids.contains(&linked_id.as_str()), "flat join result must contain the linked entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_join_level_header_limits_traversal_depth() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();

    let leaf_id = scope.id("Entity:joinlevel-leaf");
    create(client, &json!({"id": leaf_id, "type": "Building", "@context": [CORE_CONTEXT]})).await;

    let middle_id = scope.id("Entity:joinlevel-middle");
    create(
        client,
        &json!({
            "id": middle_id,
            "type": "Building",
            "linksTo": {"type": "Relationship", "object": leaf_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let root_id = scope.id("Entity:joinlevel-root");
    create(
        client,
        &json!({
            "id": root_id,
            "type": "Building",
            "linksTo": {"type": "Relationship", "object": middle_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let encoded_root = urlencoding::encode(&root_id);

    let (status, body_l1) = query_entities_with_headers(
        client,
        &format!("id={encoded_root}&type=Building"),
        &[("NGSILD-Join", "flat"), ("NGSILD-JoinLevel", "1")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let arr_1 = body_l1.as_array().expect("level-1 join must return array");
    let ids_1: Vec<&str> = arr_1.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids_1.contains(&root_id.as_str()));
    assert!(ids_1.contains(&middle_id.as_str()));
    assert!(!ids_1.contains(&leaf_id.as_str()), "NGSILD-JoinLevel: 1 must not traverse beyond one hop");

    let (status, body_l2) = query_entities_with_headers(
        client,
        &format!("id={encoded_root}&type=Building"),
        &[("NGSILD-Join", "flat"), ("NGSILD-JoinLevel", "2")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let arr_2 = body_l2.as_array().expect("level-2 join must return array");
    let ids_2: Vec<&str> = arr_2.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids_2.contains(&root_id.as_str()));
    assert!(ids_2.contains(&middle_id.as_str()));
    assert!(ids_2.contains(&leaf_id.as_str()), "NGSILD-JoinLevel: 2 must traverse two hops and include leaf");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn extension_headers_scope_via_header_filters_entities_by_scope_path() {
    let scope = TestEnvironment::from_suite("extension").await.expect("isolated test environment");
    let client = scope.client();

    let id_a = scope.id("Building:scope-a");
    let id_b = scope.id("Building:scope-b");
    let sanitized: String = scope.id_prefix().chars().filter(char::is_ascii_alphanumeric).collect();
    let scope_a = format!("/T{sanitized}/AreaA");
    let scope_b = format!("/T{sanitized}/AreaB");

    create(client, &building_entity_with_scope(&id_a, &scope_a)).await;
    create(client, &building_entity_with_scope(&id_b, &scope_b)).await;

    let (status, body) = query_entities_with_headers(client, "type=Building", &[("NGSILD-Scope", &scope_a)]).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("query result must be an array");
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids.contains(&id_a.as_str()), "entity in scope {scope_a} must appear in scoped results");
    assert!(!ids.contains(&id_b.as_str()), "entity in scope {scope_b} must be excluded");
    scope.finish().await.expect("cleanup failed");
}
