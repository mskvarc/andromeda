//! A reified Scope written through the Temporal API is held to the grammar of
//! clause 4.18.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.6 represents the Scope of a Temporal
//! Evolution as "a temporal representation of a Property", and clause 4.18
//! gives every Scope one grammar, which "shall be supported by
//! implementations": a `ScopeLevel` is a letter followed by letters, digits and
//! `_`. The broker read each value of a reified Scope instance as a scope path
//! and silently left out any value that was not one, so `"value": "/a-b"` or
//! `"value": 7` recorded no Scope at all and the request still answered with
//! success. Clause 5.5.4 answers such a value with `BadRequestData`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 answers an inadmissible value with.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Assert a refusal is the `BadRequestData` of clause 5.5.4.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// One reified Scope instance dating `value` (clause 4.5.6, annex C clause
/// C.5.16).
fn reified_scope(value: &Value) -> Value {
    json!({"type": "Property", "value": value, "observedAt": "2020-06-01T00:00:00Z"})
}

/// A Temporal Evolution of a `Vehicle` with one `speed` observation and the
/// reified Scope `scope`.
fn evolution(id: &str, scope: &Value) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "scope": [scope],
        "speed": [{"type": "Property", "value": 80, "observedAt": "2020-06-01T00:00:00Z"}],
        "@context": [CORE_CONTEXT]
    })
}

/// The Temporal Evolution of `id`, answering 200.
async fn history(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_temporal_entity(id, "", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieve {id}: {:?}", response.body);
    response.json_value().expect("a temporal entity")
}

/// Clause 4.18: a value outside the scope grammar, or no string at all, is no
/// Scope, and clause 5.5.4 refuses the Temporal Evolution carrying it; nothing
/// of the refused write is recorded.
#[tokio::test]
async fn regression_r54_create_refuses_a_reified_scope_outside_the_grammar() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (suffix, value) in [("hyphen", json!("/a-b")), ("digit-level", json!("/1st")), ("number", json!(7))] {
        let id = scope.id(&format!("Vehicle:reified-scope-{suffix}"));
        let response = client
            .create_temporal_entity(&evolution(&id, &reified_scope(&value)))
            .await
            .expect("create temporal entity request");
        assert_bad_request_data(&response, &format!("clause 4.18 grammar refuses {value}"));
        let absent = client
            .get_temporal_entity(&id, "", Some(CORE_CONTEXT))
            .await
            .expect("retrieve temporal entity request");
        assert_eq!(
            absent.status,
            StatusCode::NOT_FOUND,
            "the refused evolution {id} is not recorded: {:?}",
            absent.body
        );
    }

    scope.finish().await.expect("cleanup");
}

/// Clause 4.5.6: the reified Scope is a Property, so a Scope written as a
/// Relationship carries no Scope paths and is refused.
#[tokio::test]
async fn regression_r54_create_refuses_a_reified_scope_that_is_no_property() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:reified-scope-relationship");
    let relationship = json!({"type": "Relationship", "object": "urn:ngsi-ld:Scope:Madrid", "observedAt": "2020-06-01T00:00:00Z"});
    let response = client
        .create_temporal_entity(&evolution(&id, &relationship))
        .await
        .expect("create temporal entity request");
    assert_bad_request_data(&response, "clause 4.5.6 makes the reified Scope a Property");

    scope.finish().await.expect("cleanup");
}

/// Clause 5.6.12 appends to an existing Temporal Evolution under the same
/// rules: the refused Scope leaves the recorded history as it was.
#[tokio::test]
async fn regression_r54_append_refuses_a_reified_scope_outside_the_grammar() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:reified-scope-append");
    let created = client
        .create_temporal_entity(&evolution(&id, &reified_scope(&json!("/Madrid"))))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);
    let before = history(client, &id).await;

    let appended = client
        .raw(
            reqwest::Method::POST,
            &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(&id)),
        )
        .header("content-type", "application/ld+json")
        .json(&json!({"scope": [reified_scope(&json!("/a-b"))], "@context": [CORE_CONTEXT]}))
        .send()
        .await
        .expect("append temporal attributes request");
    assert_bad_request_data(&appended, "clause 4.18 grammar refuses /a-b on append");
    assert_eq!(history(client, &id).await, before, "the refused append records nothing");

    scope.finish().await.expect("cleanup");
}

/// The fix refuses what is no Scope, not the reified Scope itself: a path and
/// the deletion clause 4.18 spells with the NGSI-LD Null are both recorded.
#[tokio::test]
async fn regression_r54_a_reified_scope_within_the_grammar_is_recorded() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:reified-scope-valid");
    let created = client
        .create_temporal_entity(&evolution(&id, &reified_scope(&json!(["/Madrid/Gardens", "CompanyA"]))))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);

    let recorded = history(client, &id).await;
    let instances = recorded["scope"].as_array().expect("the reified Scope history");
    assert_eq!(instances.len(), 1, "{recorded}");
    assert_eq!(instances[0]["value"], json!(["/Madrid/Gardens", "CompanyA"]), "{recorded}");

    scope.finish().await.expect("cleanup");
}
