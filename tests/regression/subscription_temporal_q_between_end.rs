//! A Subscription whose `temporalQ` is a `between` without an `endTimeAt` is
//! refused at creation.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.21-1: `endTimeAt` "Cardinality shall be 1 if
//! timerel is equal to between". Clause 5.8.1.4 answers a create whose members
//! break their data type definition with `BadRequestData` (clause 5.5.4); the
//! subscription was previously accepted and its `temporalQ` silently dropped when
//! compiled for matching.
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The problem type clause 5.5.4 gives data that is not syntactically correct.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Table 5.2.21-1: a `between` `temporalQ` without `endTimeAt` is `BadRequestData`
/// naming `endTimeAt`, and the same `temporalQ` with an end is accepted.
#[tokio::test]
async fn regression_subscription_temporal_q_between_without_end_time_at_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let refused_id = scope.id("Subscription:temporal-q-between-no-end");
    let accepted_id = scope.id("Subscription:temporal-q-between-with-end");
    let body = |id: &str, temporal_q: serde_json::Value| {
        json!({
            "id": id,
            "type": "Subscription",
            "entities": [{"type": scope.entity_type("TemporalProbe")}],
            "temporalQ": temporal_q,
            "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
            "@context": [TEST_SUITE_CONTEXT]
        })
    };

    let refused = client
        .create_subscription(&body(&refused_id, json!({"timerel": "between", "timeAt": "2020-01-01T00:00:00Z"})))
        .await
        .expect("create subscription request");
    let accepted = client
        .create_subscription(&body(
            &accepted_id,
            json!({"timerel": "between", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-02-01T00:00:00Z"}),
        ))
        .await
        .expect("create subscription request");
    let deleted = client.delete_subscription(&accepted_id).await.expect("delete subscription request");

    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "refused: {:?}", refused.body);
    let problem = refused.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");
    assert!(problem.to_string().contains("endTimeAt"), "the refusal names endTimeAt: {problem}");
    assert_eq!(accepted.status, StatusCode::CREATED, "accepted: {:?}", accepted.body);
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);
    scope.finish().await.expect("cleanup failed");
}
