//! Deleting an unknown Context Source Registration Subscription is a 404.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.11.6 (Delete Context Source Registration
//! Subscription) states that "if the ... Subscription does not exist, an error
//! of type `ResourceNotFound` shall be raised", which Table 6.3.2-1 maps to HTTP
//! 404. A syntactically valid URN that names no stored subscription is exactly
//! that case: the request is well formed, so it is a not-found, not a bad
//! request.
//!
//! Corresponds to the official suite case `042_03_01`. This pins the broker's 404
//! against the well-formed-but-absent identifier.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::problem_detail::RESOURCE_NOT_FOUND;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use urlencoding::encode;

/// Clause 5.11.6: deleting a valid-but-absent Context Source Registration
/// Subscription URN answers 404 `ResourceNotFound` with a problem details body.
#[tokio::test]
async fn regression_csource_subscription_delete_unknown_is_resource_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    // A well-formed URN that names nothing this run created.
    let absent = scope.id("Subscription:csource-delete-absent");

    let deleted = client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(&absent)))
        .send()
        .await
        .expect("delete context source subscription request");
    assert_eq!(
        deleted.status,
        StatusCode::NOT_FOUND,
        "a valid-but-absent subscription is a not-found, not a bad request: {:?}",
        deleted.body
    );

    let problem = deleted.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND.as_str()), "the type is ResourceNotFound: {problem}");
    assert!(problem["title"].is_string(), "the problem carries a title: {problem}");

    scope.finish().await.expect("environment cleanup");
}
