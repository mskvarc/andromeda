//! A subscription carrying a malformed `q` is refused with `BadRequestData`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.2 gives `BadRequestData` to "input data
//! which does not meet the requirements of the operation", and a subscription's
//! `q` is such input: it is a member of the request body validated on creation
//! (clause 5.8.2 for an entity Subscription, clause 5.11.2 for a Context Source
//! Registration Subscription), not a query parameter. A syntactically malformed
//! `q` -- an unbalanced parenthesis -- fails that body validation, so both
//! resources answer `BadRequestData` rather than the `InvalidRequest` clause
//! 6.3.20 reserves for incompatible request *parameters*.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A subscription body carrying a syntactically malformed `q`.
fn subscription_with_bad_query(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "q": "(invalidQuery",
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// Assert one refusal is a 400 carrying `BadRequestData`.
fn assert_bad_request(status: StatusCode, body: &Value, scenario: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{scenario}: {body}");
    assert_eq!(body["type"], json!(BAD_REQUEST_DATA.as_str()), "{scenario}: {body}");
    assert!(body["title"].is_string(), "{scenario}: a problem carries a title: {body}");
}

/// Clause 5.8.2: an entity Subscription with a malformed `q` is `BadRequestData`.
#[tokio::test]
async fn regression_subscription_a_malformed_query_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:bad-query-entity");

    let created = client
        .create_subscription(&subscription_with_bad_query(&id))
        .await
        .expect("create subscription request");
    let body = created.json_value().expect("a refusal carries a problem details body");
    assert_bad_request(created.status, &body, "entity subscription with malformed q");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.11.2: a Context Source Registration Subscription with a malformed
/// `q` is the same `BadRequestData`, through the same body validation.
#[tokio::test]
async fn regression_csource_subscription_a_malformed_query_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:bad-query-csource");

    let created = client
        .raw(Method::POST, "csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(&subscription_with_bad_query(&id))
        .send()
        .await
        .expect("create context source subscription request");
    let body = created.json_value().expect("a refusal carries a problem details body");
    assert_bad_request(created.status, &body, "context source subscription with malformed q");

    scope.finish().await.expect("environment cleanup");
}
