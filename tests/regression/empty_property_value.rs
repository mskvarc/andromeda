//! A Property whose value is the empty array, on every Core write.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.5-1 gives `value` the data type "Any JSON
//! value as defined by IETF RFC 8259", and clause 3.1 lists "an array" among
//! the NGSI-LD Values, so `"value": []` is a Property like any other. JSON-LD
//! expansion keeps the member with no item, and drops the `null` of `[null]`
//! (clause 4.5.0), so both reach the broker as the empty array. A member with
//! no item is a value, not a missing member: a create does not answer 400
//! `MissingValue`, and an update, append, merge or batch write replaces the old
//! value.
//!
//! The other value members name at least one target, term, string or
//! geometry (clauses 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.7.1), so an empty one stays
//! refused, in a fragment too rather than silently ignored there.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type of a refused write.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The Property every test empties.
fn emptied() -> Value {
    json!({"type": "Property", "value": []})
}

/// An `EmptyProbe` carrying `tags` as written.
fn probe(id: &str, tags: &Value) -> Value {
    json!({"id": id, "type": "EmptyProbe", "tags": tags, "@context": [CORE_CONTEXT]})
}

/// An `EmptyProbe` whose `tags` hold two strings, the value each write empties.
async fn create_tagged(client: &TestClient, id: &str) {
    create(client, &probe(id, &json!({"type": "Property", "value": ["a", "b"]}))).await;
}

/// Create one entity, asserting clause 5.6.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let created = client.post_entity(body).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create {}: {:?}", body["id"], created.body);
}

/// The entity `id` in the normalized representation, or with `options`.
async fn read(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    response.json_value().expect("an entity body")
}

/// Assert that the entity `id` now holds `tags` as the empty array.
async fn assert_emptied(client: &TestClient, id: &str) {
    let entity = read(client, id, "").await;
    assert_eq!(entity["tags"], emptied(), "{entity}");
}

/// The problem type of a refused response.
fn problem_type(response: &TestResponse) -> Value {
    response.json_value().map_or(Value::Null, |problem| problem["type"].clone()) // clone: the caller owns the type it compares
}

/// Send `body` under `application/ld+json` to `path`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("request")
}

/// Clause 5.6.1 with Table 5.2.5-1: `"value": []` creates a Property holding
/// the empty array, and a retrieval answers it as written.
#[tokio::test]
async fn regression_r05_create_stores_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("EmptyProbe:create");

    create(scope.client(), &probe(&id, &emptied())).await;
    assert_emptied(scope.client(), &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.0: expansion removes the `null` of `[null]`, which leaves the
/// empty array.
#[tokio::test]
async fn regression_r05_create_reads_an_array_of_null_as_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("EmptyProbe:create-null");

    create(scope.client(), &probe(&id, &json!({"type": "Property", "value": [null]}))).await;
    assert_emptied(scope.client(), &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.2.3: the concise `"tags": []` is the Property value `[]`, and
/// the concise and simplified representations answer it as written.
#[tokio::test]
async fn regression_r05_the_concise_empty_array_is_a_property_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("EmptyProbe:concise");

    create(scope.client(), &probe(&id, &json!([]))).await;
    assert_emptied(scope.client(), &id).await;
    let concise = read(scope.client(), &id, "options=concise").await;
    assert_eq!(concise["tags"], json!([]), "{concise}");
    let simplified = read(scope.client(), &id, "options=keyValues").await;
    assert_eq!(simplified["tags"], json!([]), "{simplified}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.18: Replace Entity takes the empty array like any value.
#[tokio::test]
async fn regression_r05_replace_entity_stores_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:replace-entity");
    create_tagged(client, &id).await;

    let replaced = client
        .replace_entity(&id, &probe(&id, &emptied()), "application/ld+json", None)
        .await
        .expect("replace entity request");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.2: Update Attributes replaces the held value with the empty
/// array instead of keeping it.
#[tokio::test]
async fn regression_r05_update_attributes_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:update");
    create_tagged(client, &id).await;

    let updated = client
        .update_entity_attrs(&id, &json!({"tags": emptied(), "@context": [CORE_CONTEXT]}))
        .await
        .expect("update attributes request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.3: Append Attributes overwrites the held value with the empty
/// array.
#[tokio::test]
async fn regression_r05_append_attributes_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:append");
    create_tagged(client, &id).await;

    let appended = client
        .append_entity_attrs(&id, &json!({"tags": emptied(), "@context": [CORE_CONTEXT]}))
        .await
        .expect("append attributes request");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "{:?}", appended.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.4: a Partial Attribute Update writing `"value": []` sets it,
/// where a fragment naming no value leaves the value alone.
#[tokio::test]
async fn regression_r05_partial_attribute_update_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:partial-update");
    create_tagged(client, &id).await;

    let updated = client
        .partial_update_attr(&id, "tags", &json!({"value": [], "@context": [CORE_CONTEXT]}))
        .await
        .expect("partial attribute update request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.19: Replace Attribute takes the empty array like any value.
#[tokio::test]
async fn regression_r05_replace_attribute_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:replace-attribute");
    create_tagged(client, &id).await;

    let mut fragment = emptied();
    fragment["@context"] = json!([CORE_CONTEXT]);
    let replaced = send(
        client,
        Method::PUT,
        &format!("/ngsi-ld/v1/entities/{}/attrs/tags", urlencoding::encode(&id)),
        &fragment,
    )
    .await;
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.17 with clause 5.5.12: a merge replaces an array, which is no
/// JSON object to merge into, with the empty array.
#[tokio::test]
async fn regression_r05_merge_entity_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:merge");
    create_tagged(client, &id).await;

    let merged = client
        .merge_entity(&id, &json!({"tags": emptied(), "@context": [CORE_CONTEXT]}), None)
        .await
        .expect("merge entity request");
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "{:?}", merged.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.7: Batch Entity Creation stores the empty array.
#[tokio::test]
async fn regression_r05_batch_create_stores_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:batch-create");

    let created = client.batch_create(&json!([probe(&id, &emptied())])).await.expect("batch create request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.8: Batch Entity Upsert replaces an existing entity with one
/// holding the empty array.
#[tokio::test]
async fn regression_r05_batch_upsert_stores_the_empty_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:batch-upsert");
    create_tagged(client, &id).await;

    let upserted = client.batch_upsert(&json!([probe(&id, &emptied())])).await.expect("batch upsert request");
    assert_eq!(upserted.status, StatusCode::NO_CONTENT, "{:?}", upserted.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.9: Batch Entity Update empties the held value.
#[tokio::test]
async fn regression_r05_batch_update_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:batch-update");
    create_tagged(client, &id).await;

    let updated = client.batch_update(&json!([probe(&id, &emptied())])).await.expect("batch update request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.20: Batch Entity Merge empties the held value.
#[tokio::test]
async fn regression_r05_batch_merge_empties_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:batch-merge");
    create_tagged(client, &id).await;

    let merged = client.batch_merge(&json!([probe(&id, &emptied())])).await.expect("batch merge request");
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "{:?}", merged.body);
    assert_emptied(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// The refusals that stand: a Property naming no value (Table 5.2.5-1,
/// cardinality 1), and a Relationship, `VocabProperty`, `LanguageProperty` or
/// `GeoProperty` whose member names no target, term, string or geometry.
#[tokio::test]
async fn regression_r05_an_absent_value_or_an_empty_member_of_another_family_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let refused = [
        ("no-value", json!({"type": "Property"})),
        ("no-object", json!({"type": "Relationship", "object": []})),
        ("no-vocab", json!({"type": "VocabProperty", "vocab": []})),
        ("no-language", json!({"type": "LanguageProperty", "languageMap": {}})),
        ("no-geometry", json!({"type": "GeoProperty", "value": []})),
    ];

    for (case, tags) in refused {
        let id = scope.id(&format!("EmptyProbe:refused-{case}"));
        let response = client
            .post_entity(&probe(&id, &tags))
            .with_ld_json()
            .send()
            .await
            .expect("create entity request");
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{case}: {:?}", response.body);
        assert_eq!(problem_type(&response), json!(BAD_REQUEST_DATA), "{case}");
        let missing = client
            .get_entity(&id, "", "application/json", Some(CORE_CONTEXT))
            .await
            .expect("retrieve entity request");
        assert_eq!(missing.status, StatusCode::NOT_FOUND, "{case}: a refused create stores nothing");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.3.2: a fragment writing an empty `object` names no target, so it
/// is refused instead of answering success and keeping the old target.
#[tokio::test]
async fn regression_r05_an_empty_object_in_a_fragment_is_refused_and_keeps_the_target() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("EmptyProbe:empty-object");
    create(
        client,
        &json!({"id": id, "type": "EmptyProbe", "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1"}, "@context": [CORE_CONTEXT]}),
    )
    .await;

    let refused = client
        .partial_update_attr(&id, "owner", &json!({"object": [], "@context": [CORE_CONTEXT]}))
        .await
        .expect("partial attribute update request");
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);
    assert_eq!(problem_type(&refused), json!(BAD_REQUEST_DATA));
    let entity = read(client, &id, "").await;
    assert_eq!(entity["owner"]["object"], json!("urn:ngsi-ld:Person:1"), "{entity}");

    scope.finish().await.expect("environment cleanup");
}
