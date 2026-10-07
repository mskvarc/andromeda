#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! NGSILD-Warning header tests for distributed operations.
//!
//! Tests that the broker correctly emits warning headers when context sources
//! return errors, are unreachable, or return invalid payloads.

use reqwest::{Method, StatusCode};
use serde_json::Value;
use serial_test::serial;

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        clean_database,
        client,
        create_entity_local,
        create_registration,
        delete_entity,
        delete_registration,
        get_entity,
        jsonld_link_value,
        query_entities,
        unique_id,
        vehicle_entity,
        vehicle_registration,
        warning_header_values,
    },
};

/// Query: context source returns HTTP 500 generates warning code 299.
#[tokio::test]
#[serial]
async fn distop_entity_query_context_source_500_returns_warning_299() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-500");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-500");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::GET, "/ngsi-ld/v1/entities", 500).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query: context source unreachable generates warning code 199.
#[tokio::test]
#[serial]
async fn distop_entity_query_context_source_unreachable_returns_warning_199() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-unreachable");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-unreachable");

    let endpoint = mock.url("");
    mock.shutdown().await;

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &endpoint, None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("199")), "expected warning code 199, got: {warnings:?}");

    delete_registration(&c, &reg_id).await;
}

/// Query: context source returns invalid (non-JSON) payload generates warning code 111.
#[tokio::test]
#[serial]
async fn distop_entity_query_context_source_invalid_payload_returns_warning_111() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-invalid");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-invalid");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub_with_raw_body(Method::GET, "/ngsi-ld/v1/entities", 200, "not json at all", "text/plain")
        .await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("111")), "expected warning code 111, got: {warnings:?}");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query: context source returns 404 generates no warning (spec-compliant).
#[tokio::test]
#[serial]
async fn distop_entity_query_context_source_404_no_warning() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-404");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-404");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::GET, "/ngsi-ld/v1/entities", 404).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(warnings.is_empty(), "expected no NGSILD-Warning header for 404, got: {warnings:?}");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query: successful forwarding generates no warning header.
#[tokio::test]
#[serial]
async fn distop_entity_query_successful_forwarding_no_warning() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-success");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-success");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(
        warnings.is_empty(),
        "expected no NGSILD-Warning header for successful forwarding, got: {warnings:?}"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query: multiple context sources with mixed results generates multiple warnings.
#[tokio::test]
#[serial]
async fn distop_entity_query_multiple_sources_mixed_results_multiple_warnings() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock1 = MockServer::start().await;
    let mock2 = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-multi");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:warn-multi-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:warn-multi-2");

    let reg1 = vehicle_registration(&reg_id1, Some(&entity_id), &mock1.url(""), None);
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock1.set_stub(Method::GET, "/ngsi-ld/v1/entities", 500).await;

    let endpoint2 = mock2.url("");
    mock2.shutdown().await;

    let reg2 = vehicle_registration(&reg_id2, Some(&entity_id), &endpoint2, None);
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(
        warnings.len() >= 2,
        "expected at least 2 NGSILD-Warning headers, got {}: {warnings:?}",
        warnings.len()
    );

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock1.shutdown().await;
}

/// Query with local=true skips forwarding, generates no warnings even if CSR would fail.
#[tokio::test]
#[serial]
async fn distop_entity_query_local_true_no_warnings() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-local");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // CSR points to mock returning 500 -- but local=true should skip forwarding
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::GET, "/ngsi-ld/v1/entities", 500).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle&local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(warnings.is_empty(), "expected no NGSILD-Warning header with local=true, got: {warnings:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieve: context source returns HTTP 403 generates warning code 299.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_context_source_403_returns_warning_299() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-retrieve-403");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-retrieve-403");

    // Create local entity so we get 200 (not 404)
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 403).await;

    let (status, headers, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieve: context source returns 500 generates warning code 299.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_context_source_500_returns_warning_299() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-retrieve-500");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-retrieve-500");

    // Create local entity so we get 200 (not 404)
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 500).await;

    let (status, headers, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieve: context source unreachable generates warning code 199.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_context_source_unreachable_returns_warning_199() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-retrieve-unreachable");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-retrieve-unreachable");

    // Create local entity so we get 200 (not 404)
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let endpoint = mock.url("");
    mock.shutdown().await;

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &endpoint, None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
    assert!(warnings.iter().any(|w| w.starts_with("199")), "expected warning code 199, got: {warnings:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
}

/// Retrieve: successful forwarding generates no warning header.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_successful_forwarding_no_warning() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:warn-retrieve-success");
    let reg_id = unique_id("urn:ngsi-ld:Registration:warn-retrieve-success");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, headers, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let warnings = warning_header_values(&headers);
    assert!(
        warnings.is_empty(),
        "expected no NGSILD-Warning header for successful forwarding, got: {warnings:?}"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}
