//! Profile link header integration tests (RFC 8288 Section 3.4, NGSI-LD Clause 6.3.21).
//!
//! Naming convention: `profile_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn jsonld_link_with_profile(profile_uri: &str) -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"; profile=\"{profile_uri}\"")
}

async fn create_entity_with_link(client: &TestClient, body: &Value, link_header: &str) -> StatusCode {
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Link", link_header)
        .json(body)
        .send()
        .await
        .expect("create_entity_with_link request failed");
    response.status
}

fn testing_entity(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Testing",
        "name": {"type": "Property", "value": name}
    })
}

#[tokio::test]
async fn profile_link_header_accepted_on_entity_creation() {
    let scope = TestEnvironment::from_suite("profile").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Testing:accepted");
    let link = jsonld_link_with_profile("https://uri.etsi.org/ngsi-ld/profile/version");
    let status = create_entity_with_link(client, &testing_entity(&id, "test"), &link).await;
    assert_eq!(status, StatusCode::CREATED, "entity creation with profiled Link should succeed");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn profile_link_header_preserved_in_context_extraction() {
    let scope = TestEnvironment::from_suite("profile").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Testing:retrieve");
    let link = jsonld_link_with_profile("https://uri.etsi.org/ngsi-ld/profile/version");
    let status = create_entity_with_link(client, &testing_entity(&id, "retrieve-test"), &link).await;
    assert_eq!(status, StatusCode::CREATED);

    let response = client
        .get_entity(&id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(
        response.status,
        StatusCode::OK,
        "entity should be retrievable after creation with profiled Link"
    );
    assert_eq!(response.json_value().expect("response body must be valid JSON")["id"], id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn profile_link_header_with_standard_context_coexist() {
    let scope = TestEnvironment::from_suite("profile").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Testing:coexist");
    let link = jsonld_link_with_profile("https://uri.etsi.org/ngsi-ld/profile/version");
    let status = create_entity_with_link(client, &testing_entity(&id, "coexist-test"), &link).await;
    assert_eq!(status, StatusCode::CREATED, "Link with context and profile should work together");

    let response = client
        .get_entity(&id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json_value().expect("response body must be valid JSON")["type"], "Testing");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn profile_link_header_missing_value_returns_error() {
    let scope = TestEnvironment::from_suite("profile").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Testing:empty");
    let link = format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"; profile=");
    let status = create_entity_with_link(client, &testing_entity(&id, "bad-profile"), &link).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status.is_client_error(),
        "empty profile value should cause client error, got {status}"
    );
    scope.finish().await.expect("cleanup failed");
}
