//! Updating a Context Source Registration Subscription screens the media type
//! before it parses the body.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.4 requires a request carrying a body to
//! declare its media type, and clause 5.11.3 (Update Context Source
//! Registration Subscription) takes a JSON Merge Patch fragment. A PATCH with a
//! body but no `Content-Type` cannot be interpreted, so it is refused with 415
//! Unsupported Media Type before any JSON parsing -- the broker does not guess
//! the body is JSON. When the media type is declared as JSON and the body is
//! malformed, the parse then fails and the request is `InvalidRequest` (400),
//! the category clause 5.5.2 gives to a request that is not valid JSON.
//!
//! Corresponds to the official suite case `039_05_01`, whose fixture sends the
//! update fragment with no `Content-Type` and expects the JSON-parse failure.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail::INVALID_REQUEST};
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use urlencoding::encode;

/// Create one Context Source Registration Subscription and return its id.
async fn create_csource_subscription(client: &TestClient, scope: &TestEnvironment) -> String {
    let id = scope.id("Subscription:csource-media-type");
    let body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    });
    let created = client
        .raw(Method::POST, "csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("create context source subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    id
}

/// Clause 6.3.4: a PATCH body with no declared media type is 415 Unsupported
/// Media Type, refused before the body is read as JSON.
#[tokio::test]
async fn regression_csource_subscription_update_without_content_type_is_415() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_csource_subscription(client, &scope).await;

    let updated = client
        .raw(Method::PATCH, &format!("csourceSubscriptions/{}", encode(&id)))
        .body(br#"{"description": "no content type"}"#.to_vec())
        .send()
        .await
        .expect("update without content type request");
    assert_eq!(
        updated.status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "a body with no media type is refused before parsing: {:?}",
        updated.body
    );

    client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(&id)))
        .send()
        .await
        .expect("delete context source subscription request");
    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.5.2: a PATCH declaring a JSON media type but carrying a malformed
/// body is `InvalidRequest` (400) -- the media type was accepted, the JSON was
/// not.
#[tokio::test]
async fn regression_csource_subscription_update_with_malformed_json_is_invalid_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_csource_subscription(client, &scope).await;

    let updated = client
        .raw(Method::PATCH, &format!("csourceSubscriptions/{}", encode(&id)))
        .header("content-type", "application/json")
        .body(br#"{"description": "unterminated"#.to_vec())
        .send()
        .await
        .expect("update with malformed json request");
    assert_eq!(
        updated.status,
        StatusCode::BAD_REQUEST,
        "a declared JSON body that is malformed is a bad request: {:?}",
        updated.body
    );
    let problem = updated.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(INVALID_REQUEST.as_str()), "the type is InvalidRequest: {problem}");

    client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(&id)))
        .send()
        .await
        .expect("delete context source subscription request");
    scope.finish().await.expect("environment cleanup");
}
