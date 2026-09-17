//! Partial update of a temporal attribute instance distinguishes a missing
//! attribute name from a well-formed name that the entity does not carry.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.14 (Modify Attribute Instance) is reached
//! at `PATCH /temporal/entities/{entityId}/attrs/{attrId}/{instanceId}`. Two
//! failure shapes on the `{attrId}` segment are told apart:
//!
//! - An empty `{attrId}` segment is not a valid Attribute name (clause 4.6.2),
//!   so the request is malformed and refused with `BadRequestData` (400). It is
//!   not a 405: the route exists and the method is allowed; the path value is
//!   the problem.
//! - A well-formed name that the entity does not carry is `ResourceNotFound`
//!   (404): clause 5.6.14.4 raises it "if the Attribute ... does not exist".
//!
//! Corresponds to the official suite case `016_02_06`, which sends an empty
//! attribute-name segment and whose 405 expectation clause 5.6.14.4 does not
//! support.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{
    context::CORE_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND},
};
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use urlencoding::encode;

/// The attribute-instance fragment clause 5.6.14 patches with.
fn fragment() -> Value {
    json!({"value": 99, "@context": [CORE_CONTEXT]})
}

/// Create one temporal entity carrying a single `speed` instance, and return
/// its id.
async fn create_temporal_vehicle(client: &TestClient, scope: &TestEnvironment) -> String {
    let id = scope.id("Vehicle:temporal-attr-absent");
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 60, "observedAt": "2025-01-01T00:00:00Z"}],
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_temporal_entity(&body).await.expect("create temporal entity request");
    assert!(
        created.status == StatusCode::CREATED || created.status == StatusCode::NO_CONTENT,
        "create temporal entity: {:?}",
        created.body
    );
    id
}

/// Clause 5.6.14.4: an empty attribute-name path segment is a malformed request
/// (`BadRequestData`, 400), not a 405.
#[tokio::test]
async fn regression_temporal_instance_empty_attribute_name_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_temporal_vehicle(client, &scope).await;

    // `.../attrs//{instanceId}` -- the attribute-name segment is empty.
    let path = format!("temporal/entities/{}/attrs//{}", encode(&id), encode("urn:ngsi-ld:instance:placeholder"));
    let response = client
        .raw(Method::PATCH, &path)
        .header("content-type", "application/ld+json")
        .json(&fragment())
        .send()
        .await
        .expect("empty attribute-name request");
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "an empty attribute name is a malformed request: {:?}",
        response.body
    );
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA.as_str()), "the type is BadRequestData: {problem}");

    client.delete_temporal_entity(&id).await.expect("delete temporal entity request");
    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.14.4: a well-formed attribute name the entity does not carry is
/// `ResourceNotFound` (404).
#[tokio::test]
async fn regression_temporal_instance_absent_attribute_name_is_resource_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_temporal_vehicle(client, &scope).await;

    // `temperature` is well formed but the entity carries only `speed`.
    let path = format!("temporal/entities/{}/attrs/temperature/{}", encode(&id), encode("urn:ngsi-ld:instance:absent"));
    let response = client
        .raw(Method::PATCH, &path)
        .header("content-type", "application/ld+json")
        .json(&fragment())
        .send()
        .await
        .expect("absent attribute-name request");
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "a well-formed but absent attribute is a not-found: {:?}",
        response.body
    );
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND.as_str()), "the type is ResourceNotFound: {problem}");

    client.delete_temporal_entity(&id).await.expect("delete temporal entity request");
    scope.finish().await.expect("environment cleanup");
}
