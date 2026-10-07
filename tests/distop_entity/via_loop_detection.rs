#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Via header loop detection tests.
//!
//! Tests the broker's ability to detect and prevent forwarding loops using the Via header
//! and contextSourceAlias.

use reqwest::{Method, StatusCode};
use serde_json::Value;
use serial_test::serial;

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        clean_database,
        client,
        create_registration,
        delete_registration,
        jsonld_link_value,
        query_entities,
        query_entities_with_via,
        unique_id,
        vehicle_entity,
        vehicle_registration,
        vehicle_registration_with_alias,
    },
};

/// CSR alias in Via header causes that CSR to be skipped (prevents loop).
#[tokio::test]
#[serial]
async fn distop_entity_via_csr_alias_in_header_skips_csr() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-skip");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-skip");

    let reg = vehicle_registration_with_alias(&reg_id, Some(&entity_id), &mock.url(""), None, "cs-broker-A");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, _) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "1.1 cs-broker-A").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 0, "mock should not have been called when CSR alias is in Via");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// CSR alias not in Via header allows forwarding to proceed.
#[tokio::test]
#[serial]
async fn distop_entity_via_different_alias_allows_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-forward");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-forward");

    let reg = vehicle_registration_with_alias(&reg_id, Some(&entity_id), &mock.url(""), None, "cs-broker-A");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, _) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "1.1 cs-broker-B").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 1, "mock should have been called when CSR alias is not in Via");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// No Via header means forwarding proceeds normally.
#[tokio::test]
#[serial]
async fn distop_entity_via_no_header_allows_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-none");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-none");

    let reg = vehicle_registration_with_alias(&reg_id, Some(&entity_id), &mock.url(""), None, "cs-broker-A");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, _) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 1, "mock should have been called without Via header");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Substring match does not cause false positive (broker1 != broker10).
#[tokio::test]
#[serial]
async fn distop_entity_via_substring_no_false_positive() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-substring");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-substring");

    let reg = vehicle_registration_with_alias(&reg_id, Some(&entity_id), &mock.url(""), None, "broker1");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    // Via header contains "broker10" which is NOT "broker1"
    let (status, _, _) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "1.1 broker10").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 1, "mock should have been called -- substring match must not trigger");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// CSR without alias is always forwarded regardless of Via header content.
#[tokio::test]
#[serial]
async fn distop_entity_via_no_alias_always_forwards() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-no-alias");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-no-alias");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, _) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "1.1 whatever").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 1, "mock should have been called -- CSR has no alias");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Broker's own identity in Via header prevents all forwarding (self-loop detection).
#[tokio::test]
#[serial]
async fn distop_entity_via_self_loop_prevents_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();

    // No CSR needed -- the broker's own identity check happens before CSR matching
    // The broker identity is urn:ngsi-ld:ContextSourceIdentity:andromeda by default
    let (status, _, body) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "1.1 urn:ngsi-ld:ContextSourceIdentity:andromeda").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.is_empty(), "self-loop should prevent all forwarding");
}

/// Multiple aliases in Via header: CSR skipped if any match.
#[tokio::test]
#[serial]
async fn distop_entity_via_multiple_aliases_any_match_skips() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-multi");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-multi");

    let reg = vehicle_registration_with_alias(&reg_id, Some(&entity_id), &mock.url(""), None, "cs-broker-B");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, _) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "1.1 cs-broker-A, 1.1 cs-broker-B").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 0, "mock should not have been called when CSR alias is in Via");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Via header with HTTP version prefix parsed correctly.
#[tokio::test]
#[serial]
async fn distop_entity_via_http_version_prefix_parsed() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:via-version");
    let reg_id = unique_id("urn:ngsi-ld:Registration:via-version");

    let reg = vehicle_registration_with_alias(&reg_id, Some(&entity_id), &mock.url(""), None, "cs-broker-A");
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, _) = query_entities_with_via(&c, "type=Vehicle", &jsonld_link_value(), "2.0 cs-broker-A").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities");
    pretty_assertions::assert_eq!(count, 0, "mock should not have been called regardless of HTTP version");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}
