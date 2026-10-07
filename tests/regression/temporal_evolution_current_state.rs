//! The current state a Temporal API write leaves behind.
//!
//! ETSI GS CIM 009 v1.9.1 clauses 5.6.11.4 and 5.6.12.4 add every instance "to
//! the target Temporal Evolution of an Entity", while current state holds one
//! instance per Attribute instance (clause 4.5.5: "There can only be one
//! default Attribute instance for an Attribute with a given Attribute name in
//! any request or response"), the most current one (clause 4.5.5.3: the most
//! recent `observedAt`, then the most recent `modifiedAt`). A client may create
//! an Entity through the Temporal API and then delete one of its Attributes
//! through the Core API, so a Temporal API create brings the Entity into
//! current state.
//!
//! A newly created Temporal Evolution is not stored as the Entity's Core
//! document whole: `GET /entities/{id}` answers the one current instance of
//! each attribute, not every historical instance as a same-attribute array
//! under the default `datasetId`, and the current-attribute projection a Core
//! `q` reads agrees with that body. A Temporal API append advances current state
//! only when the instance is more recent, and a backfill does not rewind what
//! `q` reads.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// One Property observation of `value` at `observed_at`.
fn observation(value: i64, observed_at: &str) -> Value {
    json!({"type": "Property", "value": value, "observedAt": observed_at})
}

/// A Temporal Evolution of `v` holding three instances, the latest written
/// last.
fn series(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "v": [
            observation(10, "2020-01-01T00:00:00Z"),
            observation(200, "2020-01-02T00:00:00Z"),
            observation(50, "2020-01-03T00:00:00Z")
        ],
        "@context": CORE_CONTEXT
    })
}

/// POST /temporal/entities, asserting clause 6.20.3.1's status.
async fn post_temporal(client: &TestClient, body: &Value, expected: StatusCode) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, expected, "{:?}", response.body);
}

/// GET /entities/{id} in `options`.
async fn core(client: &TestClient, id: &str, query: &str) -> (StatusCode, Value) {
    let response = client
        .get_entity(id, query, "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    let status = response.status;
    (status, response.json_value().unwrap_or(Value::Null))
}

/// The identifiers a Core query of `entity_type` with `query` answers.
async fn core_ids(client: &TestClient, entity_type: &str, query: &str) -> Vec<String> {
    let response = client
        .query_entities(&format!("type={entity_type}{query}"), "application/json", Some(CORE_CONTEXT))
        .await
        .expect("query entities request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response
        .json_value()
        .expect("a query answers JSON")
        .as_array()
        .expect("a collection")
        .iter()
        .map(|entity| entity["id"].as_str().expect("an id").to_owned())
        .collect()
}

/// The values of every recorded instance of `v`, in the order the temporal
/// representation lists them.
async fn history(client: &TestClient, id: &str) -> Vec<i64> {
    let response = client
        .get_temporal_entity(
            id,
            "attrs=v&timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=modifiedAt",
            Some(CORE_CONTEXT),
        )
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json_value().expect("a temporal answer is JSON");
    let instances: Vec<&Value> = match &body["v"] {
        Value::Array(instances) => instances.iter().collect(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => vec![other],
    };
    let mut values: Vec<i64> = instances.iter().map(|instance| instance["value"].as_i64().expect("an integer value")).collect();
    values.sort_unstable();
    values
}

/// Clause 4.5.5.3: a Temporal Evolution created for an unknown identifier
/// brings the Entity into current state with the most recently observed
/// instance alone -- whatever order the series was written in -- and a Core
/// `q` agrees with the Core body. Every instance stays in the Temporal
/// Evolution.
#[tokio::test]
async fn regression_temporal_evolution_current_state_a_temporal_create_holds_the_most_current_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TemporalCreate");
    let id = scope.id(&format!("{entity_type}:1"));

    // The most recent observation is written first.
    let series = json!({
        "id": id,
        "type": entity_type,
        "v": [
            observation(50, "2020-01-03T00:00:00Z"),
            observation(10, "2020-01-01T00:00:00Z"),
            observation(200, "2020-01-02T00:00:00Z")
        ],
        "@context": CORE_CONTEXT
    });
    post_temporal(scope.client(), &series, StatusCode::CREATED).await;

    let (status, body) = core(scope.client(), &id, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["v"].is_object(), "clause 4.5.5: one default instance, not the series: {body}");
    assert_eq!(body["v"]["value"], json!(50), "the most recently observed instance stands: {body}");
    assert!(body["v"].get("instanceId").is_none(), "an instanceId belongs to the Temporal Evolution: {body}");
    let (_, simplified) = core(scope.client(), &id, "options=keyValues").await;
    assert_eq!(simplified["v"], json!(50), "keyValues is the bare number: {simplified}");
    assert_eq!(core_ids(scope.client(), &entity_type, "&q=v==50").await, vec![id.clone()]);
    assert!(
        core_ids(scope.client(), &entity_type, "&q=v==200").await.is_empty(),
        "Core q agrees with the Core body"
    );
    assert_eq!(
        history(scope.client(), &id).await,
        vec![10, 50, 200],
        "the Temporal Evolution holds every instance"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.12.4 with clause 4.5.5.3: an appended observation more recent
/// than current state advances it -- body and `q` together -- and a backfill of
/// an older one leaves it as it was. Every instance stays in the Temporal
/// Evolution.
#[tokio::test]
async fn regression_temporal_evolution_current_state_a_newer_append_advances_and_a_backfill_does_not() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TemporalAppend");
    let id = scope.id(&format!("{entity_type}:1"));
    let created = scope
        .client()
        .post_entity(
            &json!({"id": id, "type": entity_type, "v": {"type": "Property", "value": 5, "observedAt": "2021-01-01T00:00:00Z"}, "@context": CORE_CONTEXT}),
        )
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let appended = scope
        .client()
        .raw(
            reqwest::Method::POST,
            &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(&id)),
        )
        .header("content-type", "application/ld+json")
        .json(&json!({"v": [observation(200, "2030-01-01T00:00:00Z")], "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("append temporal attributes request");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "{:?}", appended.body);
    let (_, body) = core(scope.client(), &id, "").await;
    assert_eq!(body["v"]["value"], json!(200), "the newer observation stands: {body}");
    assert_eq!(core_ids(scope.client(), &entity_type, "&q=v==200").await, vec![id.clone()]);
    assert_eq!(core_ids(scope.client(), &entity_type, "&q=v==5").await, Vec::<String>::new());

    // A backfill through the clause 5.6.11 update branch.
    post_temporal(
        scope.client(),
        &json!({"id": id, "type": entity_type, "v": [observation(1, "2019-01-01T00:00:00Z")], "@context": CORE_CONTEXT}),
        StatusCode::NO_CONTENT,
    )
    .await;
    let (_, body) = core(scope.client(), &id, "").await;
    assert_eq!(body["v"]["value"], json!(200), "a backfill does not rewind current state: {body}");
    assert!(core_ids(scope.client(), &entity_type, "&q=v==1").await.is_empty(), "nor what Core q reads");
    assert_eq!(history(scope.client(), &id).await, vec![1, 5, 200], "the Temporal Evolution holds all three");

    scope.finish().await.expect("cleanup failed");
}

/// A day's series through the Temporal API, then
/// a Core upsert restating the latest reading. The Core Entity carries that one
/// reading -- a bare number under keyValues -- and a Core `q` agrees with it.
#[tokio::test]
async fn regression_temporal_evolution_current_state_the_series_then_upsert_pattern_keeps_one_core_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("CrowdFlow");
    let id = scope.id(&format!("{entity_type}:1"));

    post_temporal(scope.client(), &series(&id, &entity_type), StatusCode::CREATED).await;
    let upserted = scope
        .client()
        .raw(reqwest::Method::POST, "/ngsi-ld/v1/entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&json!([{"id": id, "type": entity_type, "v": observation(50, "2020-01-03T00:00:00Z"), "@context": CORE_CONTEXT}]))
        .send()
        .await
        .expect("batch upsert request");
    assert!(upserted.status.is_success(), "{:?}", upserted.body);

    let (status, body) = core(scope.client(), &id, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["v"]["value"], json!(50), "{body}");
    assert!(body["v"].is_object(), "clause 4.5.5: one default instance, not the series: {body}");
    let (_, simplified) = core(scope.client(), &id, "options=keyValues").await;
    assert_eq!(simplified["v"], json!(50), "keyValues is the bare number: {simplified}");
    assert_eq!(core_ids(scope.client(), &entity_type, "&q=v==50").await, vec![id.clone()]);
    assert_eq!(core_ids(scope.client(), &entity_type, "&q=v==200").await, Vec::<String>::new());

    scope.finish().await.expect("cleanup failed");
}

/// An Entity created through the Temporal
/// API has its Attribute deleted through the Core API (clause 5.6.5), and the
/// Temporal Evolution records exactly one deletion of the one current instance
/// (clauses 4.5.7 and 4.5.8).
#[tokio::test]
async fn regression_temporal_evolution_current_state_a_core_deletion_after_a_temporal_create_records_one_tombstone() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TemporalThenCore");
    let id = scope.id(&format!("{entity_type}:1"));
    post_temporal(scope.client(), &series(&id, &entity_type), StatusCode::CREATED).await;

    let deleted = scope
        .client()
        .raw(reqwest::Method::DELETE, &format!("/ngsi-ld/v1/entities/{}/attrs/v", urlencoding::encode(&id)))
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("a valid context Link"))
        .send()
        .await
        .expect("delete attribute request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let response = scope
        .client()
        .get_temporal_entity(&id, "timeproperty=deletedAt", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json_value().expect("a temporal answer is JSON");
    let tombstones: Vec<&Value> = match &body["v"] {
        Value::Array(instances) => instances.iter().collect(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => vec![other],
    };
    assert_eq!(tombstones.len(), 1, "one deletion of the one current instance: {body}");
    assert!(tombstones[0].get("deletedAt").is_some(), "{body}");
    assert_eq!(tombstones[0]["value"], json!("urn:ngsi-ld:null"), "{body}");

    scope.finish().await.expect("cleanup failed");
}
