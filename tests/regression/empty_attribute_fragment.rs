//! The Attribute Fragment of Replace Attribute is a whole instance; that of
//! Partial Attribute Update need not name any member.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.19.4 runs the clause 5.5.4 validation of
//! the payload before any forwarding, and the payload it replaces the instance
//! with is an instance of its own: Table 5.2.5-1 makes the `value` of a Property
//! mandatory, and the other families' value members likewise. A replacement
//! naming no value member -- `{}`, or only a `unitCode` -- is therefore refused
//! with `BadRequestData` (clause 5.5.2), a Fragment declaring no family being
//! read as a Property (clause 4.5.2). Clause 5.6.4.4 only patches the members
//! the Fragment names (clause 5.5.8, an IETF RFC 7396 merge patch under clause
//! 5.4), so the same `{}` there stays the 204 no-op recorded in
//! `docs/empty-fragment-writes.md`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.2 names for input the operation cannot accept.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Create the vehicle every case writes over.
async fn seed(client: &TestClient, id: &str) {
    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 50, "unitCode": "KMH"},
            "@context": [CORE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// The entity as stored, system attributes included.
async fn stored(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .get_entity(id, "options=sysAttrs", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    retrieved.json_value().expect("a retrieved entity is JSON")
}

/// Send `body` as `application/json` to `method` `/entities/{id}/attrs/speed`.
async fn write_speed(client: &TestClient, method: Method, id: &str, body: &Value) -> TestResponse {
    client
        .raw(method, &format!("/ngsi-ld/v1/entities/{}/attrs/speed", urlencoding::encode(id)))
        .header("content-type", "application/json")
        .body(body.to_string().into_bytes())
        .send()
        .await
        .expect("attribute write request")
}

/// Clauses 5.6.19.4 and 5.2.5: a replacement without the value member its
/// family requires is a `BadRequestData`, and the stored instance is untouched.
#[tokio::test]
async fn regression_empty_attribute_fragment_replacement_without_value_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:replacement-without-value");
    seed(client, &id).await;
    let before = stored(client, &id).await;

    for replacement in [json!({}), json!({"unitCode": "C62"})] {
        let refused = write_speed(client, Method::PUT, &id, &replacement).await;

        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "PUT {replacement}: {:?}", refused.body);
        let problem = refused.json_value().expect("a refusal carries a problem details body");
        assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "PUT {replacement}: {problem}");
        assert_eq!(stored(client, &id).await, before, "PUT {replacement} changed the entity");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.4, 5.5.8 and 5.6.4.4: the same `{}` on Partial Attribute Update
/// names no member to change, so it answers 204 and leaves `speed` -- and the
/// entity's `modifiedAt` -- as it was.
#[tokio::test]
async fn regression_empty_attribute_fragment_partial_update_stays_a_no_op() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:partial-update-without-member");
    seed(client, &id).await;
    let before = stored(client, &id).await;

    let answer = write_speed(client, Method::PATCH, &id, &json!({})).await;

    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{:?}", answer.body);
    let after = stored(client, &id).await;
    assert_eq!(after["speed"], before["speed"]);
    assert_eq!(after, before);

    scope.finish().await.expect("cleanup failed");
}
