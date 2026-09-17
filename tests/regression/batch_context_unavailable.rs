//! A batch create whose shared `@context` cannot be retrieved reports the
//! failure per entity, not as a top-level error.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.6 makes an unretrievable remote JSON-LD
//! `@context` an error of type `LdContextNotAvailable`, which Table 6.3.2-1 maps
//! to HTTP 504. Clause 5.6.7.5 states that batch entity creation returns "207
//! Multi-Status ... where a `BatchOperationResult` ... is returned", with a
//! `BatchEntityError` per entity that failed. The two clauses together settle
//! the batch case: the unretrievable `@context` is the same failure for every
//! entity in the array, so it is reported once per entity inside a 207, not as
//! one 504 for the whole request.
//!
//! The broker screened the shared `@context` first and aborted the whole batch
//! with a top-level 504 the moment it could not be resolved, so the per-entity
//! error array clause 5.6.7.5 mandates was never produced. It now projects the
//! one refusal onto every submitted entity and answers 207. A single-entity
//! create with the same `@context` still answers a top-level 504, which is
//! clause 5.6.1's behaviour and is unchanged.
//!
//! Corresponds to the official suite case `043_01_04`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::problem_detail::LD_CONTEXT_NOT_AVAILABLE;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A `Link` header naming a remote `@context` at an address nothing answers on,
/// so its resolution fails with a connection refusal rather than a slow
/// timeout (clause 5.5.6).
const UNREACHABLE_CONTEXT_LINK: &str =
    r#"<http://127.0.0.1:19998/unreachable-context.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;

/// One Building entity carrying no inline `@context`, so the sole context is the
/// unreachable one named in the `Link` header.
fn building(identifier: &str) -> Value {
    json!({
        "id": identifier,
        "type": "Building",
        "name": {"type": "Property", "value": "Reichstag"}
    })
}

/// Clause 5.6.7.5: a batch create of two entities whose shared `@context` is
/// unretrievable answers 207, with an `LdContextNotAvailable` `BatchEntityError`
/// for each submitted entity.
#[tokio::test]
async fn regression_batch_context_unavailable_reports_a_per_entity_error_in_a_207() {
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
        StatusCode::MULTI_STATUS,
        "an unretrievable @context is a per-entity failure, not a top-level one: {:?}",
        created.body
    );

    let result = created.json_value().expect("a 207 carries a BatchOperationResult");
    let errors = result["errors"].as_array().expect("the result carries an errors array");
    assert_eq!(errors.len(), 2, "every submitted entity is reported: {result}");
    assert!(result["success"].as_array().is_some_and(Vec::is_empty), "no entity was created: {result}");

    let reported: Vec<&str> = errors.iter().filter_map(|error| error["entityId"].as_str()).collect();
    for identifier in [&first, &second] {
        assert!(reported.contains(&identifier.as_str()), "{identifier} is named in the errors: {result}");
    }
    for error in errors {
        assert_eq!(
            error["error"]["type"],
            json!(LD_CONTEXT_NOT_AVAILABLE.as_str()),
            "each error is LdContextNotAvailable: {error}"
        );
        assert!(error["error"]["title"].is_string(), "each error carries a title: {error}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.1: a single-entity create against the same unretrievable
/// `@context` still fails the operation as a whole, with the Table 6.3.2-1
/// status 504 for `LdContextNotAvailable`. The batch fix does not touch it.
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
