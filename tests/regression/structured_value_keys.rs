//! The keys of a JSON object inside a Property value.
//!
//! ETSI GS CIM 009 v1.9.1 clause 3.1 and Table 5.2.5-1 make a Property value
//! any JSON value, and clause 4.6.3 obliges a broker to support every JSON
//! native type; the name grammar of clause 4.6.2 restricts Entity Type,
//! Property and Relationship names only. The broker held the keys inside a
//! value to that grammar, so `{"opening hours": "9-5"}`, `{"id": "abc"}` (the
//! core context aliases `id` to `@id`) and `{"type": "Postal Address"}` were
//! refused with 400. They are stored and read back as written.
//!
//! Clause 4.6.4 requires the content of a value back as written. Two keys
//! JSON-LD expansion cannot keep were accepted and silently changed: a key
//! that has the form of a keyword without being one (`@foo`) was dropped, and
//! an empty key or `type` came back as `ngsi-ld:default-context/`. Both are
//! refused as `BadRequestData`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The core context the entities are written and read under.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The problem type clause 5.5.4 gives data that is not syntactically correct.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// One `Shop` carrying the single attribute `p`.
fn shop(id: &str, attribute: &Value) -> Value {
    json!({"id": id, "type": "Shop", "p": attribute, "@context": CORE_CONTEXT})
}

/// Create `entity`, asserting 201.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create {}: {:?}", entity["id"], created.body);
}

/// The attribute `p` of the entity `id` in the normalized representation.
async fn attribute(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.json_value().expect("a JSON entity")["p"].clone() // clone: the attribute outlives the response body
}

/// Assert a refusal is the `BadRequestData` of clause 5.5.4, naming `at`.
fn assert_bad_request_data(response: &TestResponse, at: &str, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(detail.contains(at), "{scenario}: the detail names {at}: {problem}");
}

/// Create a `Shop` whose `p` is `written` and assert it reads back unchanged.
async fn assert_round_trip(scope: &TestEnvironment, name: &str, written: &Value) {
    let client = scope.client();
    let id = scope.id(&format!("Shop:{name}"));
    create(client, &shop(&id, written)).await;
    assert_eq!(attribute(client, &id).await, *written, "{name}");
}

/// Clause 4.6.2 restricts attribute names only: a key with a space is data.
#[tokio::test]
async fn regression_structured_value_keys_a_key_with_a_space_reads_back_as_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    assert_round_trip(
        &scope,
        "spaced-key",
        &json!({"type": "Property", "value": {"opening hours": "9-5", "nested": {"a b": [{"c d": 1}, {"e f": 2}]}}}),
    )
    .await;
    scope.finish().await.expect("cleanup failed");
}

/// An NGSI-LD document has no base IRI, so a relative `id` inside a value is
/// kept as written.
#[tokio::test]
async fn regression_structured_value_keys_a_relative_id_reads_back_as_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    assert_round_trip(&scope, "relative-id", &json!({"type": "Property", "value": {"id": "abc", "name": "x"}})).await;
    scope.finish().await.expect("cleanup failed");
}

/// A `type` inside a value is data, whatever text it holds.
#[tokio::test]
async fn regression_structured_value_keys_a_type_with_a_space_reads_back_as_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    assert_round_trip(
        &scope,
        "spaced-type",
        &json!({"type": "Property", "value": {"type": "Postal Address", "street": "x"}}),
    )
    .await;
    scope.finish().await.expect("cleanup failed");
}

/// IETF RFC 7946 section 6.1 lets a geometry carry any foreign member.
#[tokio::test]
async fn regression_structured_value_keys_a_geometry_foreign_member_with_a_space_reads_back_as_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    assert_round_trip(
        &scope,
        "spaced-foreign-member",
        &json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.5, 2.5], "plot name": "7"}}),
    )
    .await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.17: a merge replaces the members it names inside a structured
/// value, keys with spaces included.
#[tokio::test]
async fn regression_structured_value_keys_a_merge_keeps_a_key_with_a_space() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Shop:merged-spaced-key");
    create(
        client,
        &shop(&id, &json!({"type": "Property", "value": {"opening hours": "9-5", "closed on": "Sunday"}})),
    )
    .await;

    let merged = client
        .merge_entity(&id, &json!({"p": {"type": "Property", "value": {"opening hours": "10-6"}}}), Some(CORE_CONTEXT))
        .await
        .expect("merge entity request");
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "{:?}", merged.body);
    assert_eq!(
        attribute(client, &id).await,
        json!({"type": "Property", "value": {"opening hours": "10-6", "closed on": "Sunday"}})
    );

    scope.finish().await.expect("cleanup failed");
}

/// The Temporal Evolution keeps the value as written too.
#[tokio::test]
async fn regression_structured_value_keys_the_temporal_evolution_reads_back_a_key_with_a_space() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Shop:temporal-spaced-key");
    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "Shop",
            "p": [{"type": "Property", "value": {"opening hours": "9-5"}, "observedAt": "2024-01-01T00:00:00Z"}],
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let response = client
        .get_temporal_entity(&id, "", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let evolution = response.json_value().expect("a JSON evolution");
    let instances = evolution["p"].as_array().map(Vec::as_slice).unwrap_or_default();
    let values: Vec<&Value> = instances.iter().map(|instance| &instance["value"]).collect();
    assert_eq!(values, vec![&json!({"opening hours": "9-5"})], "{evolution}");

    scope.finish().await.expect("cleanup failed");
}

/// JSON-LD 1.1 expansion ignores a key that has the form of a keyword without
/// being one; clause 4.6.4 forbids losing it, so it is refused wherever it
/// stands.
#[tokio::test]
async fn regression_structured_value_keys_a_keyword_form_key_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let in_value = client
        .post_entity(&shop(
            &scope.id("Shop:keyword-form-in-value"),
            &json!({"type": "Property", "value": {"@foo": "x", "name": "y"}}),
        ))
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&in_value, "/p/value/@foo", "a keyword-form key inside a value");

    let on_attribute = client
        .post_entity(&shop(
            &scope.id("Shop:keyword-form-on-attribute"),
            &json!({"type": "Property", "value": 1, "@foo": 2}),
        ))
        .send()
        .await
        .expect("create entity request");
    assert_eq!(
        on_attribute.status,
        StatusCode::BAD_REQUEST,
        "a keyword-form key on an attribute: {:?}",
        on_attribute.body
    );

    let id = scope.id("Shop:keyword-form-on-append");
    create(client, &shop(&id, &json!({"type": "Property", "value": 1}))).await;
    let appended = client
        .append_entity_attrs(&id, &json!({"@foo": {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}))
        .await
        .expect("append request");
    assert_bad_request_data(&appended, "/@foo", "a keyword-form key at the top of a fragment");

    scope.finish().await.expect("cleanup failed");
}

/// An empty key or `type` inside a value would come back as
/// `ngsi-ld:default-context/` (clause 4.6.4), so it is refused.
#[tokio::test]
async fn regression_structured_value_keys_an_empty_key_or_type_inside_a_value_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let empty_key = client
        .post_entity(&shop(&scope.id("Shop:empty-key"), &json!({"type": "Property", "value": {"": "x"}})))
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&empty_key, "/p/value/", "an empty key inside a value");

    let empty_type = client
        .post_entity(&shop(
            &scope.id("Shop:empty-type"),
            &json!({"type": "Property", "value": {"type": "", "street": "x"}}),
        ))
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&empty_type, "/p/value/type", "an empty type inside a value");

    scope.finish().await.expect("cleanup failed");
}
