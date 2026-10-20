//! ETSI GS CIM 009 v1.9.1 clause 5.5.6 makes an unretrievable remote JSON-LD `@context` "referenced by an incoming request" an error of type `LdContextNotAvailable`, which Table 6.3.2-1 maps to HTTP 504. The `Link` header context of an `application/json` batch belongs to the request and holds for every entity, so the request fails as a whole with a 504, as a single-entity create does (clause 5.6.1). An `@context` an entity of an `application/ld+json` batch carries itself is that entity's: clause 5.6.7.5 answers 207 with a `BatchEntityError` for every entity naming it.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::problem_detail::LD_CONTEXT_NOT_AVAILABLE;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A `Link` header naming a remote `@context` at an address nothing answers on, so its resolution fails with a connection refusal rather than a slow timeout (clause 5.5.6).
const UNREACHABLE_CONTEXT_LINK: &str =
    r#"<http://127.0.0.1:19998/unreachable-context.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;

/// One Building entity carrying no inline `@context`, so the sole context is the unreachable one named in the `Link` header.
fn building(identifier: &str) -> Value {
    json!({
        "id": identifier,
        "type": "Building",
        "name": {"type": "Property", "value": "Reichstag"}
    })
}

/// Clause 5.5.6: a batch create whose `Link` context is unretrievable answers
/// 504 `LdContextNotAvailable` for the request and creates no entity.
#[tokio::test]
async fn regression_batch_context_unavailable_a_link_context_fails_the_whole_batch_with_a_504() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let first = scope.id("Building:batch-context-unavailable-first");
    let second = scope.id("Building:batch-context-unavailable-second");

    let created = client
        .raw(Method::POST, "entityOperations/create")
        .header("content-type", "application/json")
        .header("link", UNREACHABLE_CONTEXT_LINK)
        .json(&json!([building(&first), building(&second)]))
        .send()
        .await
        .expect("batch creation request");
    assert_eq!(
        created.status,
        StatusCode::GATEWAY_TIMEOUT,
        "the request's own context fails the request: {:?}",
        created.body
    );
    let problem = created.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(LD_CONTEXT_NOT_AVAILABLE.as_str()), "{problem}");

    for identifier in [&first, &second] {
        let read = client
            .raw(Method::GET, &format!("entities/{identifier}"))
            .send()
            .await
            .expect("retrieve request");
        assert_eq!(read.status, StatusCode::NOT_FOUND, "{identifier} was not created: {:?}", read.body);
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.1: a single-entity create against the same unretrievable
/// `@context` still fails the operation as a whole, with the Table 6.3.2-1
/// status 504 for `LdContextNotAvailable`.
#[tokio::test]
async fn regression_batch_context_unavailable_single_create_stays_a_top_level_504() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client: &TestClient = scope.client();
    let identifier = scope.id("Building:single-context-unavailable");

    let created = client
        .raw(Method::POST, "entities/")
        .header("content-type", "application/json")
        .header("link", UNREACHABLE_CONTEXT_LINK)
        .json(&building(&identifier))
        .send()
        .await
        .expect("single creation request");
    assert_eq!(
        created.status,
        StatusCode::GATEWAY_TIMEOUT,
        "a single create fails the whole operation: {:?}",
        created.body
    );

    let problem = created.json_value().expect("a refusal carries a problem details body");
    assert_eq!(
        problem["type"],
        json!(LD_CONTEXT_NOT_AVAILABLE.as_str()),
        "the top-level problem is LdContextNotAvailable: {problem}"
    );
    assert!(problem["title"].is_string(), "the problem carries a title: {problem}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.5.4 judges a payload "syntactically correct, according to the @context", so a single-entity create whose `@context` cannot be retrieved is `LdContextNotAvailable` even when its payload would also be refused: the context is admitted before anything is read under it.
#[tokio::test]
async fn regression_batch_context_unavailable_an_unreachable_context_outranks_a_refused_payload() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let identifier = scope.id("Building:context-unavailable-before-payload");

    let created = client
        .raw(Method::POST, "entities")
        .header("content-type", "application/json")
        .header("link", UNREACHABLE_CONTEXT_LINK)
        .json(&json!({
            "id": identifier,
            "type": "Building",
            "name": {"type": "NotAnAttributeType", "value": "Reichstag"}
        }))
        .send()
        .await
        .expect("creation request");
    assert_eq!(created.status, StatusCode::GATEWAY_TIMEOUT, "{:?}", created.body);
    let problem = created.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(LD_CONTEXT_NOT_AVAILABLE.as_str()), "{problem}");

    scope.finish().await.expect("environment cleanup");
}
