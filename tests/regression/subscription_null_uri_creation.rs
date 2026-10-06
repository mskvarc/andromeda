//! The NGSI-LD Null written as a URI is refused when a Subscription is created.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.1 makes an NGSI-LD Null outside the update
//! clauses 5.5.8 and 5.5.12 `BadRequestData`, and clause 4.5.0 spells that Null
//! `urn:ngsi-ld:null` in the places URI strings are valid, `jsonldContext`
//! among them. A create has no member to delete, but the broker read the URI as
//! an absent member and created the subscription.
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The problem type clause 5.5.4 gives data that is not syntactically correct.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Clause 5.2.1: a create writing `urn:ngsi-ld:null` as `jsonldContext` is
/// `BadRequestData` naming the member, and the subscription is not created.
#[tokio::test]
async fn regression_subscription_null_uri_creation_a_null_jsonld_context_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:null-jsonld-context");
    let body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": scope.entity_type("NullContextProbe")}],
        "jsonldContext": "urn:ngsi-ld:null",
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });

    let refused = client.create_subscription(&body).await.expect("create subscription request");
    let retrieved = client.retrieve_subscription(&id, None).await.expect("retrieve subscription request");

    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);
    let problem = refused.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");
    let detail = problem["detail"].as_str().expect("a problem names its detail");
    assert!(detail.contains("jsonldContext"), "the member is named: {detail}");
    assert_eq!(retrieved.status, StatusCode::NOT_FOUND, "nothing was created: {:?}", retrieved.body);
    scope.finish().await.expect("cleanup failed");
}
