#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use andromeda_test_fixtures::context::CORE_CONTEXT;
use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::helpers::*;
use serde_json::{Number, json};
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn batch_create_minimal_entities_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_01_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_01_01b");

    let e1 = building_minimal(&id1);
    let e2 = building_minimal(&id2);
    let entities = json!([e1, e2]);

    let (status, body) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "batch create should return 201");

    let uris = body.as_array().expect("response should be array of URIs");
    pretty_assertions::assert_eq!(uris.len(), 2);

    let (status, _, _) = get_entity(&c, &id1, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let (status, _, _) = get_entity(&c, &id2, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn batch_create_with_properties_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_01_02a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_01_02b");

    let e1 = building_simple_attributes(&id1);
    let e2 = building_simple_attributes(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_create_preserves_exact_numbers_and_classifies_range_failures_per_entity() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let fraction_id = unique_id("urn:ngsi-ld:Building:batch_exact_fraction");
    let integer_id = unique_id("urn:ngsi-ld:Building:batch_exact_integer");
    let rejected_id = unique_id("urn:ngsi-ld:Building:batch_out_of_range");
    let fraction = Number::from_str("0.180000000000000005").expect("exact JSON fraction");
    let integer = Number::from_str("18446744073709551616").expect("exact JSON integer");
    let out_of_range = Number::from_str("1e-16384").expect("syntactically valid JSON number");
    let entities = json!([
        {"id": fraction_id, "type": "Building", "reading": {"type": "Property", "value": fraction}, "@context": [CORE_CONTEXT]},
        {"id": integer_id, "type": "Building", "reading": {"type": "Property", "value": integer}, "@context": [CORE_CONTEXT]},
        {"id": rejected_id, "type": "Building", "reading": {"type": "Property", "value": out_of_range}, "@context": [CORE_CONTEXT]}
    ]);

    let (status, body) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "batch result: {body}");
    pretty_assertions::assert_eq!(body["success"].as_array().map(Vec::len), Some(2));
    pretty_assertions::assert_eq!(body["errors"].as_array().map(Vec::len), Some(1));

    let (status, _, fraction_body) = get_entity(&c, &fraction_id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        serde_json::to_string(&fraction_body)
            .expect("fraction entity JSON")
            .contains("0.180000000000000005")
    );
    let (status, _, integer_body) = get_entity(&c, &integer_id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        serde_json::to_string(&integer_body)
            .expect("integer entity JSON")
            .contains("18446744073709551616")
    );
    let (status, _, _) = get_entity(&c, &rejected_id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn batch_create_with_relationships_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_01_03a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_01_03b");

    let e1 = building_relationship(&id1);
    let e2 = building_relationship(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["locatedAt"]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn batch_create_with_relationship_of_property_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_01_04a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_01_04b");

    let e1 = building_relationship_of_property(&id1);
    let e2 = building_relationship_of_property(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn batch_create_with_existing_entity_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:003_02_01_exist");
    let pre = building_minimal(&existing_id);
    let (status, _, _) = create_entity(&c, &pre).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let new_id1 = unique_id("urn:ngsi-ld:Building:003_02_01a");
    let new_id2 = unique_id("urn:ngsi-ld:Building:003_02_01b");
    let e1 = building_minimal(&new_id1);
    let e2 = building_minimal(&new_id2);
    let e3 = building_minimal(&existing_id);
    let entities = json!([e1, e2, e3]);

    let (status, body) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "mixed result should return 207");

    let success = body["success"].as_array().expect("should have success array");
    let errors = body["errors"].as_array().expect("should have errors array");
    pretty_assertions::assert_eq!(success.len(), 2, "2 entities should succeed");
    pretty_assertions::assert_eq!(errors.len(), 1, "1 entity should fail");
}

#[tokio::test]
async fn batch_create_invalid_json_returns_400() {
    let c = client();
    let (status, body) = request_raw(
        &c,
        Method::POST,
        "/entityOperations/create",
        b"[{\"id\":\"urn:ngsi-ld:Building:invalid\",,\"type\":\"Building\"}]",
        Some("application/ld+json"),
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["type"].as_str().is_some(), "should contain error type");
}

#[tokio::test]
async fn batch_create_empty_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", b"[]", Some("application/ld+json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn batch_create_json_with_link_header_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_04_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_04_01b");

    let e1 = building_simple_attributes_json(&id1);
    let e2 = building_simple_attributes_json(&id2);
    let entities = json!([e1, e2]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/json"), true).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn batch_create_json_with_link_header_retrieval_returns_200() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:003_04_02");
    let e1 = building_simple_attributes_json(&id);
    let entities = json!([e1]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/json"), true).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_create_default_context_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_05_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_05_01b");

    let e1 = building_simple_attributes_json(&id1);
    let e2 = building_simple_attributes_json(&id2);
    let entities = json!([e1, e2]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, _) = get_entity(&c, &id1, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn batch_create_default_context_retrieval_returns_200() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:003_05_02");
    let e1 = building_simple_attributes_json(&id);
    let entities = json!([e1]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_create_json_with_context_in_body_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:003_06_01");
    let e1 = building_minimal(&id);
    let entities = json!([e1]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "@context with JSON CT should cause per-entity error");
}

#[tokio::test]
#[serial]
async fn batch_create_ld_json_with_context_returns_201() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_07_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_07_01b");

    let e1 = building_simple_attributes(&id1);
    let e2 = building_simple_attributes(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn batch_create_ld_json_with_context_retrieval_returns_200() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:003_07_02");
    let e1 = building_simple_attributes(&id);
    let entities = json!([e1]);

    let (status, _) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
    pretty_assertions::assert_eq!(body["subCategory"]["value"], "tourism");
}

#[tokio::test]
#[serial]
async fn batch_create_ld_json_without_context_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:003_08_01");
    let e1 = building_simple_attributes_json(&id);
    let entities = json!([e1]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/ld+json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "missing @context with ld+json should fail per-entity");
}

#[tokio::test]
#[serial]
async fn batch_create_ld_json_with_link_header_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:003_09_01");
    let e1 = building_minimal(&id);
    let entities = json!([e1]);
    let payload = serde_json::to_vec(&entities).unwrap();

    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/create", &payload, Some("application/ld+json"), true).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "Link + ld+json should return 400");
}

#[tokio::test]
#[serial]
async fn batch_create_duplicate_ids_in_batch_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:003_10_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:003_10_01b");

    let e1 = building_minimal(&id1);
    let e2 = building_minimal(&id2);
    let e3 = building_minimal(&id1);
    let entities = json!([e1, e2, e3]);

    let (status, body) = batch_create(&c, &entities).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "duplicate IDs should return 207");

    let success = body["success"].as_array().expect("should have success array");
    pretty_assertions::assert_eq!(success.len(), 2, "2 unique entities should succeed");
}
