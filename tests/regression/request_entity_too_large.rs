//! A payload over the size limit is refused with a problem detail.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 requires "Request Entity too large"
//! (413) "when the HTTP input data stream provided by a client was too large
//! i.e. too many bytes", and clause 5.5.3 makes every error body a problem
//! detail. A 65.7 MiB batch upsert was answered 413 with the framework's plain
//! text "Failed to buffer the request body: length limit exceeded"; the same
//! body sent to Create Entity was answered 400 `InvalidRequest`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::environment::TestEnvironment;
use reqwest::{Method, StatusCode};
use serde_json::Value;

/// The broker's default `http.max_request_body`, which the test stacks keep.
const MAX_REQUEST_BODY: usize = 64 * 1024 * 1024;

/// One body-taking operation and the media type its payload announces.
struct BodyTakingOperation {
    /// The request method.
    method: Method,
    /// The request target, relative to the NGSI-LD endpoint.
    path: &'static str,
    /// The payload's `Content-Type`.
    content_type: &'static str,
}

/// Every family of payload reader: an entity representation, a batch array, a
/// hosted `@context` document, and the plain JSON bodies of the entity map and
/// tenant updates.
const OPERATIONS: [BodyTakingOperation; 5] = [
    BodyTakingOperation {
        method: Method::POST,
        path: "entities",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::POST,
        path: "entityOperations/upsert",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::POST,
        path: "jsonldContexts",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::PATCH,
        path: "entityMaps/urn:ngsi-ld:EntityMap:too-large",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::PATCH,
        path: "tenants/too-large",
        content_type: "application/json",
    },
];

/// Every body-taking operation refuses a payload one byte over the limit with
/// the 413 of clause 6.3.2, as a problem detail that names the limit.
#[tokio::test]
async fn regression_request_entity_too_large_every_payload_reader_refuses_with_a_problem_detail() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let oversized = vec![b' '; MAX_REQUEST_BODY + 1];

    let mut failures = Vec::new();
    for BodyTakingOperation { method, path, content_type } in OPERATIONS {
        let request = format!("{method} {path}");
        let response = client
            .raw(method, path)
            .header("content-type", content_type)
            .body(oversized.clone()) // clone: every request owns its payload
            .send()
            .await
            .expect("the broker answers");

        let problem: Option<Value> = response.json_value().ok();
        let detail = problem.as_ref().and_then(|problem| problem["detail"].as_str()).unwrap_or_default();
        let well_formed = response.status == StatusCode::PAYLOAD_TOO_LARGE
            && problem
                .as_ref()
                .is_some_and(|problem| problem["type"] == "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge" && problem["status"] == 413)
            && detail.contains(&format!("{MAX_REQUEST_BODY} bytes"));
        if !well_formed {
            failures.push(format!("{request}: {} {}", response.status, String::from_utf8_lossy(&response.body)));
        }
    }
    assert!(
        failures.is_empty(),
        "oversized payloads not refused as a 413 problem detail:\n{}",
        failures.join("\n")
    );

    scope.finish().await.expect("cleanup failed");
}

/// A payload at the limit is not refused as too large: whatever the operation
/// makes of its content, the size alone is admitted.
#[tokio::test]
async fn regression_request_entity_too_large_a_payload_at_the_limit_is_admitted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let response = client
        .raw(Method::POST, "entities")
        .header("content-type", "application/json")
        .body(vec![b' '; MAX_REQUEST_BODY])
        .send()
        .await
        .expect("the broker answers");

    assert_ne!(response.status, StatusCode::PAYLOAD_TOO_LARGE, "{:?}", String::from_utf8_lossy(&response.body));

    scope.finish().await.expect("cleanup failed");
}
