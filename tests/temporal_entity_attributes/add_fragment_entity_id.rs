//! The `id` an Add Attributes Fragment carries names the Entity the request
//! path targets.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.4: a Fragment's `id` is "optional for
//! certain bindings where it can be determined from the operation signature.
//! It shall be equal to the id of the target (mutated) NGSI-LD element."
//! Clause 5.6.12.3 names the target by the request path, and clause 5.6.12.4
//! runs the clause 5.5.4 validation, whose failures are `BadRequestData`.
//!
//! The broker let the Fragment's `id` win over the path. Once the Entity had
//! been deleted through the Core API (clause 5.6.6 leaves its Temporal
//! Evolution standing), the added instances brought current state back into
//! being under the path's key but with the Fragment's `id`: retrieving
//! `/entities/A` answered `"id": A`, while a query matching it answered the
//! same Entity as `"id": B`.

use crate::{
    COMPOUND_CONTEXT,
    add_temporal_attrs,
    client,
    create_temporal_entity,
    get_temporal_entity,
    request_temporal_attrs,
    vehicle_temporal_representation,
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};

const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The clause 5.6.12.3 Fragment adding one `speed` instance with value 99,
/// naming `id` when one is given.
fn speed_fragment(id: Option<&str>) -> Value {
    let mut fragment = json!({
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 99, "observedAt": "2020-09-02T12:00:00Z"}],
        "@context": [COMPOUND_CONTEXT]
    });
    if let (Some(id), Some(members)) = (id, fragment.as_object_mut()) {
        members.insert("id".to_owned(), json!(id));
    }
    fragment
}

/// Create the standard temporal Vehicle `id` (two `speed` instances), and
/// assert clause 5.6.11 answered 201.
async fn create_vehicle(c: &Client, id: &str) {
    assert_eq!(create_temporal_entity(c, &vehicle_temporal_representation(id)).await, StatusCode::CREATED);
}

/// Delete the Entity `id` through the Core API (clause 5.6.6), which leaves
/// its Temporal Evolution standing, and assert it answered 204.
async fn delete_current_state(c: &Client, id: &str) {
    let (status, body) = request_temporal_attrs(c, Method::DELETE, &format!("/entities/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "core delete {id}: {body}");
}

/// The current state of `id` (clause 5.7.1): the status and the body.
async fn current_state(c: &Client, id: &str) -> (StatusCode, Value) {
    request_temporal_attrs(c, Method::GET, &format!("/entities/{id}"), None).await
}

/// The `id` of every Vehicle a clause 5.7.2 query whose `idPattern` matches
/// `id` alone answers. The query renders the stored Entity, where retrieval
/// by the path answers under the identifier it was asked for, so a stored
/// `@id` other than the path's surfaces here.
async fn queried_ids(c: &Client, id: &str) -> Vec<Value> {
    let path = format!("/entities?type=https://ngsi-ld-test-suite/context%23Vehicle&idPattern=%5E{id}%24");
    let (status, body) = request_temporal_attrs(c, Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK, "query {id}: {body}");
    body.as_array()
        .expect("a query answers an array")
        .iter()
        .map(|entity| entity["id"].clone()) // clone: the assertion owns the identifiers it compares
        .collect()
}

/// Add `fragment` to the Temporal Evolution of `id`, and assert clause 5.5.4
/// refused it with 400 `BadRequestData`.
async fn assert_refused(c: &Client, id: &str, fragment: &Value) {
    let (status, body) = add_temporal_attrs(c, id, fragment).await;
    let problem: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::BAD_REQUEST, "add attrs naming another entity: {problem}");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA));
}

/// The `value` of every `speed` instance the Temporal Evolution of `id` holds,
/// in ascending order.
async fn speed_values(c: &Client, id: &str) -> Vec<i64> {
    let (status, body) = get_temporal_entity(c, id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK, "retrieve temporal {id}: {body}");
    let mut values = body["speed"]
        .as_array()
        .expect("speed holds instances")
        .iter()
        .filter_map(|instance| instance["value"].as_i64())
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}

/// Clause 5.4: a Fragment naming another Entity is refused, and nothing is
/// added to the target's evolution.
#[tokio::test]
async fn temporal_attrs_add_naming_another_entity_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:add_id_other");
    let other = scope.id("Vehicle:add_id_other_named");
    create_vehicle(&c, &id).await;

    assert_refused(&c, &id, &speed_fragment(Some(&other))).await;

    assert_eq!(speed_values(&c, &id).await, vec![80, 120]);
    let (status, entity) = current_state(&c, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(entity["id"], json!(id));
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.4, after a Core delete (clause 5.6.6): the refusal keeps the
/// Fragment's `id` from bringing current state back into being under the
/// path. Before the fix this answered 204, and a query answered the restored
/// Entity under the other `id`.
#[tokio::test]
async fn temporal_attrs_add_naming_another_entity_after_core_delete_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:add_id_deleted");
    let other = scope.id("Vehicle:add_id_deleted_named");
    create_vehicle(&c, &id).await;
    delete_current_state(&c, &id).await;

    assert_refused(&c, &id, &speed_fragment(Some(&other))).await;

    let (status, entity) = current_state(&c, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no current state was brought back: {entity}");
    let (status, entity) = current_state(&c, &other).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the other entity was not created: {entity}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.12.3: a Fragment naming no `id` is addressed by the path, so
/// after a Core delete the added instance brings the path's Entity back.
#[tokio::test]
async fn temporal_attrs_add_after_core_delete_restores_the_path_entity() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:add_id_restored");
    create_vehicle(&c, &id).await;
    delete_current_state(&c, &id).await;

    let (status, body) = add_temporal_attrs(&c, &id, &speed_fragment(None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&body));

    let (status, entity) = current_state(&c, &id).await;
    assert_eq!(status, StatusCode::OK, "{entity}");
    assert_eq!(entity["id"], json!(id));
    assert_eq!(queried_ids(&c, &id).await, vec![json!(id)]);
    assert!(speed_values(&c, &id).await.contains(&99));
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.12.4 targets the evolution whose id "is equivalent to the one
/// passed as a parameter"; RFC 8141 compares the scheme and the namespace
/// identifier without regard to case, so an equivalent spelling is the
/// target's own `id`.
#[tokio::test]
async fn temporal_attrs_add_naming_an_equivalent_spelling_succeeds() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:add_id_equivalent");
    let equivalent = id.replacen("urn:ngsi-ld:", "URN:NGSI-LD:", 1);
    assert_ne!(equivalent, id, "the spelling differs");
    create_vehicle(&c, &id).await;

    let (status, body) = add_temporal_attrs(&c, &id, &speed_fragment(Some(&equivalent))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&body));

    assert_eq!(speed_values(&c, &id).await, vec![80, 99, 120]);
    let (status, entity) = current_state(&c, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(entity["id"], json!(id));
    scope.finish().await.expect("cleanup failed");
}
