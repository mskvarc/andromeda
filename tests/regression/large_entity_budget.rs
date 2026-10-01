//! Large entities are budgeted, and those within the budget are served whole.
//!
//! A Create Entity of a one-million-vertex polygon held the broker's memory for
//! seconds and gigabytes, and its deletion was killed for memory. ETSI GS CIM
//! 009 v1.9.1 sets no size for an entity or a geometry; clause 6.3.2 answers a
//! request whose "HTTP input data stream [...] was too large" with 413
//! `RequestEntityTooLarge`, and clause 5.5.6 makes a query "so complex that
//! cannot be resolved by an NGSI-LD system" a `TooComplexQuery`, which Table
//! 6.3.2-1 answers 403. The broker therefore budgets the JSON values of one
//! request (1,000,000 by default), the JSON values of one entity outside its
//! geometries and JSON literals (100,000) and the positions of one geometry
//! (100,000), refusing what exceeds them before it is expanded, while a
//! geometry at the budget is created, read, queried and deleted intact.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::{f64::consts::TAU, fmt::Write, time::Duration};

/// The default budget of positions in one geometry.
pub(crate) const GEOMETRY_POSITIONS: usize = 100_000;

/// The default budget of JSON values in one entity.
const ENTITY_JSON_VALUES: usize = 100_000;

/// The default budget of JSON values in one request.
const REQUEST_JSON_VALUES: usize = 1_000_000;

/// How long one exchange of a large payload may take.
pub(crate) const LARGE_EXCHANGE: Duration = Duration::from_secs(300);

/// How long the owed deletion notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(30);

/// How long the absence of a second notification is watched for.
const QUIET: Duration = Duration::from_secs(8);

/// The problem type of a refused oversized request (clause 6.3.2).
pub(crate) const REQUEST_ENTITY_TOO_LARGE: &str = "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge";

/// The problem type of a query the broker does not resolve (clause 5.5.6).
const TOO_COMPLEX_QUERY: &str = "https://uri.etsi.org/ngsi-ld/errors/TooComplexQuery";

/// The core context the `application/ld+json` bodies name.
pub(crate) const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The point every ring is drawn around, inside each ring.
const CENTRE: (f64, f64) = (14.55, 46.05);

/// The closed ring of `positions` positions around [`CENTRE`]: `positions - 1`
/// vertices on a circle and the first repeated to close it.
pub(crate) fn ring(positions: usize) -> Vec<(f64, f64)> {
    let vertices = u32::try_from(positions - 1).expect("a ring of fewer than four billion vertices");
    let on_circle = |k: u32| {
        let angle = TAU * f64::from(k) / f64::from(vertices);
        let round = |ordinate: f64| (ordinate * 1e9).round() / 1e9;
        (round(CENTRE.0 + 0.05 * angle.cos()), round(CENTRE.1 + 0.05 * angle.sin()))
    };
    (0..vertices).chain([0]).map(on_circle).collect()
}

/// The `GeoJSON` Polygon text of `ring`, written without building a JSON tree.
pub(crate) fn polygon_text(ring: &[(f64, f64)]) -> String {
    let mut text = String::with_capacity(ring.len() * 28 + 64);
    text.push_str(r#"{"type":"Polygon","coordinates":[["#);
    for (index, (longitude, latitude)) in ring.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        write!(text, "[{longitude},{latitude}]").expect("writing to a string succeeds");
    }
    text.push_str("]]}");
    text
}

/// The `coordinates` text of the Polygon `polygon_text` writes for `ring`.
fn polygon_coordinates(ring: &[(f64, f64)]) -> String {
    let polygon = polygon_text(ring);
    polygon
        .strip_prefix(r#"{"type":"Polygon","coordinates":"#)
        .and_then(|rest| rest.strip_suffix('}'))
        .expect("the polygon text ends in its coordinates")
        .to_owned()
}

/// The entity text of `id` of `entity_type` whose `location` is `geometry`, a
/// JSON text.
pub(crate) fn located_entity(id: &str, entity_type: &str, geometry: &str) -> String {
    format!(r#"{{"id":"{id}","type":"{entity_type}","location":{{"type":"GeoProperty","value":{geometry}}}}}"#)
}

/// The entity text of `id` of `entity_type` whose `location` is `geometry`, a
/// JSON text, and whose `state` Property is `state`.
pub(crate) fn located_entity_in_state(id: &str, entity_type: &str, geometry: &str, state: &str) -> String {
    format!(r#"{{"id":"{id}","type":"{entity_type}","state":{{"type":"Property","value":"{state}"}},"location":{{"type":"GeoProperty","value":{geometry}}}}}"#)
}

/// Subscribe `client` to the deletions of `entity_type` through `triggers`,
/// for entities whose removed state passes `q`, notifying `adapter`.
pub(crate) async fn subscribe_to_deletions(
    client: &TestClient,
    adapter: &NotificationAdapter,
    subscription: &str,
    entity_type: &str,
    triggers: &[&str],
    q: &str,
) {
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription,
            "type": "Subscription",
            "entities": [{"type": entity_type}],
            "q": q,
            "notificationTrigger": triggers,
            "notification": {"endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
}

/// Send `body` to `path` of `client` as `application/json`.
pub(crate) async fn send_json(client: &TestClient, method: Method, path: &str, body: String) -> TestResponse {
    client
        .raw(method, path)
        .header("Content-Type", "application/json")
        .body(body.into_bytes())
        .timeout(LARGE_EXCHANGE)
        .send()
        .await
        .expect("the large request is answered")
}

/// Assert `response` is the problem `problem_type` with `status` whose detail
/// names every fragment.
pub(crate) fn assert_problem(response: &TestResponse, status: StatusCode, problem_type: &str, fragments: &[&str]) {
    let body = response.json_value().expect("a problem detail is JSON");
    assert_eq!(response.status, status, "{body}");
    assert_eq!(body["type"], problem_type, "{body}");
    assert_eq!(body["status"], status.as_u16(), "{body}");
    let detail = body["detail"].as_str().expect("the problem names its detail");
    for fragment in fragments {
        assert!(detail.contains(fragment), "{fragment} in {detail}");
    }
}

/// The positions of the `location` of `entity`, a normalized entity.
pub(crate) fn location_ring(entity: &Value) -> Vec<(f64, f64)> {
    entity["location"]["value"]["coordinates"][0]
        .as_array()
        .expect("the stored Polygon carries its ring")
        .iter()
        .map(|position| (position[0].as_f64().expect("a longitude"), position[1].as_f64().expect("a latitude")))
        .collect()
}

/// Assert no entity `id` exists.
pub(crate) async fn assert_absent(client: &TestClient, id: &str) {
    let read = client.get_entity(id, "", "application/json", None).await.expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::NOT_FOUND, "{:?}", read.body);
}

#[tokio::test]
async fn regression_a_n12_a_body_over_the_json_value_budget_is_413_and_creates_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Probe:values");
    let zeros = vec!["0"; REQUEST_JSON_VALUES + 1].join(",");
    let body = format!(
        r#"{{"id":"{id}","type":"{}","n":{{"type":"Property","value":[{zeros}]}}}}"#,
        scope.entity_type("Probe")
    );

    let refused = send_json(scope.client(), Method::POST, "/ngsi-ld/v1/entities", body).await;

    assert_problem(
        &refused,
        StatusCode::PAYLOAD_TOO_LARGE,
        REQUEST_ENTITY_TOO_LARGE,
        &[&REQUEST_JSON_VALUES.to_string()],
    );
    assert_eq!(refused.json_value().expect("a problem")["instance"], "/ngsi-ld/v1/entities");
    assert_absent(scope.client(), &id).await;
    scope.finish().await.expect("cleanup");
}

/// The temporal `location` instance text of `geometry`, observed at `at`.
fn temporal_location(geometry: &str, at: &str) -> String {
    format!(r#"{{"type":"GeoProperty","value":{geometry},"observedAt":"{at}"}}"#)
}

/// The Temporal API writes are budgeted like the Core API's (clause 6.3.2):
/// a geometry past the position budget is refused with 413 whether it
/// creates a Temporal Evolution (clause 5.6.11), appends to one (clause
/// 5.6.12) or modifies one of its instances (clause 5.6.14), and nothing is
/// written.
#[tokio::test]
async fn regression_a_n12_every_temporal_write_of_a_geometry_over_the_budget_is_413() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Probe");
    let oversized = polygon_text(&ring(GEOMETRY_POSITIONS + 1));
    let budget = GEOMETRY_POSITIONS.to_string();

    let refused_id = scope.id("Probe:temporal-refused");
    let refused = send_json(
        scope.client(),
        Method::POST,
        "/ngsi-ld/v1/temporal/entities",
        format!(
            r#"{{"id":"{refused_id}","type":"{entity_type}","location":[{}]}}"#,
            temporal_location(&oversized, "2026-01-01T00:00:00Z")
        ),
    )
    .await;
    assert_problem(&refused, StatusCode::PAYLOAD_TOO_LARGE, REQUEST_ENTITY_TOO_LARGE, &[&budget, "location"]);
    let absent = scope
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{refused_id}"))
        .send()
        .await
        .expect("retrieve request");
    assert_eq!(absent.status, StatusCode::NOT_FOUND, "{:?}", absent.body);

    let id = scope.id("Probe:temporal");
    let small = polygon_text(&ring(10));
    let created = send_json(
        scope.client(),
        Method::POST,
        "/ngsi-ld/v1/temporal/entities",
        format!(
            r#"{{"id":"{id}","type":"{entity_type}","location":[{}]}}"#,
            temporal_location(&small, "2026-01-01T00:00:00Z")
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let appended = send_json(
        scope.client(),
        Method::POST,
        &format!("/ngsi-ld/v1/temporal/entities/{id}/attrs"),
        format!(r#"{{"location":[{}]}}"#, temporal_location(&oversized, "2026-01-02T00:00:00Z")),
    )
    .await;
    assert_problem(&appended, StatusCode::PAYLOAD_TOO_LARGE, REQUEST_ENTITY_TOO_LARGE, &[&budget, "location"]);

    let history = scope
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{id}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("retrieve request");
    assert_eq!(history.status, StatusCode::OK, "{:?}", history.body);
    let history = history.json_value().expect("a temporal entity");
    let instances = history["location"].as_array().expect("the location history");
    assert_eq!(instances.len(), 1, "the refused append wrote nothing: {history}");
    let instance_id = instances[0]["instanceId"].as_str().expect("the instance names its instanceId");

    let modified = send_json(
        scope.client(),
        Method::PATCH,
        &format!("/ngsi-ld/v1/temporal/entities/{id}/attrs/location/{instance_id}"),
        format!(r#"{{"location":[{}]}}"#, temporal_location(&oversized, "2026-01-01T00:00:00Z")),
    )
    .await;
    assert_problem(&modified, StatusCode::PAYLOAD_TOO_LARGE, REQUEST_ENTITY_TOO_LARGE, &[&budget, "location"]);
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn regression_a_n12_a_geometry_over_the_position_budget_is_413_in_either_encoding() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Probe");
    let polygon = polygon_text(&ring(GEOMETRY_POSITIONS + 1));
    // Clause 4.7.2 lets a GeoProperty carry its geometry as a JSON string.
    let encoded = serde_json::to_string(&polygon).expect("a string serializes");

    for (local, geometry) in [("Probe:object", polygon.as_str()), ("Probe:string", encoded.as_str())] {
        let id = scope.id(local);
        let refused = send_json(
            scope.client(),
            Method::POST,
            "/ngsi-ld/v1/entities",
            located_entity(&id, &entity_type, geometry),
        )
        .await;
        assert_problem(
            &refused,
            StatusCode::PAYLOAD_TOO_LARGE,
            REQUEST_ENTITY_TOO_LARGE,
            &["location", &GEOMETRY_POSITIONS.to_string()],
        );
        assert_absent(scope.client(), &id).await;
    }
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn regression_a_n12_a_batch_refuses_only_its_oversized_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Probe");
    let big = scope.id("Probe:batch-big");
    let small = scope.id("Probe:batch-small");
    let body = format!(
        r#"[{},{{"id":"{small}","type":"{entity_type}","n":{{"type":"Property","value":1}}}}]"#,
        located_entity(&big, &entity_type, &polygon_text(&ring(GEOMETRY_POSITIONS + 1)))
    );

    let answered = send_json(scope.client(), Method::POST, "/ngsi-ld/v1/entityOperations/upsert", body).await;

    let result = answered.json_value().expect("a batch result is JSON");
    assert_eq!(answered.status, StatusCode::MULTI_STATUS, "{result}");
    assert_eq!(result["success"], json!([small]), "{result}");
    let errors = result["errors"].as_array().expect("the batch lists its errors");
    assert_eq!(errors.len(), 1, "{result}");
    assert_eq!(errors[0]["entityId"], big.as_str());
    assert_eq!(errors[0]["error"]["status"], 413);
    assert_eq!(errors[0]["error"]["type"], REQUEST_ENTITY_TOO_LARGE);
    let read = scope
        .client()
        .get_entity(&small, "", "application/json", None)
        .await
        .expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
    assert_absent(scope.client(), &big).await;
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn regression_a_n12_an_entity_over_the_value_budget_is_413() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Probe:entity-values");
    let numbers = vec!["7"; ENTITY_JSON_VALUES + 1].join(",");
    let body = format!(
        r#"{{"id":"{id}","type":"{}","n":{{"type":"Property","value":[{numbers}]}}}}"#,
        scope.entity_type("Probe")
    );

    let refused = send_json(scope.client(), Method::POST, "/ngsi-ld/v1/entities", body).await;

    assert_problem(
        &refused,
        StatusCode::PAYLOAD_TOO_LARGE,
        REQUEST_ENTITY_TOO_LARGE,
        &[&ENTITY_JSON_VALUES.to_string()],
    );
    assert_absent(scope.client(), &id).await;
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn regression_a_n12_a_geometry_at_the_budget_round_trips_and_deletes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Probe");
    let id = scope.id("Probe:at-budget");
    let sent = ring(GEOMETRY_POSITIONS);

    let created = send_json(
        scope.client(),
        Method::POST,
        "/ngsi-ld/v1/entities",
        located_entity(&id, &entity_type, &polygon_text(&sent)),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let read = scope
        .client()
        .get_entity(&id, "", "application/json", None)
        .await
        .expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::OK);
    assert!(
        location_ring(&read.json_value().expect("an entity")) == sent,
        "the stored ring is the ring sent"
    );

    let query = format!(
        "type={entity_type}&georel=intersects&geometry=Point&coordinates=%5B{},{}%5D",
        CENTRE.0, CENTRE.1
    );
    let found = scope
        .client()
        .query_entities(&query, "application/json", None)
        .await
        .expect("query entities request");
    assert_eq!(found.status, StatusCode::OK);
    let found = found.json_value().expect("a query result");
    let ids: Vec<&str> = found
        .as_array()
        .expect("an entity array")
        .iter()
        .filter_map(|entity| entity["id"].as_str())
        .collect();
    assert_eq!(ids, vec![id.as_str()]);

    let deleted = scope.client().delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
    assert_absent(scope.client(), &id).await;
    let after = scope
        .client()
        .query_entities(&format!("type={entity_type}"), "application/json", None)
        .await
        .expect("query entities request");
    assert_eq!(after.status, StatusCode::OK, "the broker serves on after the deletion");
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn regression_a_n12_a_query_reference_geometry_over_budget_is_too_complex() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let coordinates = polygon_coordinates(&ring(GEOMETRY_POSITIONS + 1));
    // Table 5.2.13-1: the coordinates may be encoded as a string, which is
    // counted from its text before it is parsed.
    let encoded = serde_json::to_string(&coordinates).expect("a string serializes");
    for coordinates in [coordinates.as_str(), encoded.as_str()] {
        let body = format!(
            r#"{{"type":"Query","entities":[{{"type":"{}"}}],"geoQ":{{"geometry":"Polygon","coordinates":{coordinates},"georel":"intersects"}}}}"#,
            scope.entity_type("Probe")
        );

        let refused = send_json(scope.client(), Method::POST, "/ngsi-ld/v1/entityOperations/query", body).await;

        assert_problem(&refused, StatusCode::FORBIDDEN, TOO_COMPLEX_QUERY, &[&GEOMETRY_POSITIONS.to_string()]);
    }
    scope.finish().await.expect("cleanup");
}

/// Clause 6.3.2: a subscription's `geoQ` past the position budget is a written
/// input larger than the broker processes, refused with 413 in either encoding
/// of its coordinates, and no subscription is created.
#[tokio::test]
async fn regression_a_n12_a_subscription_geo_q_over_the_budget_is_413() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let coordinates = polygon_coordinates(&ring(GEOMETRY_POSITIONS + 1));
    let encoded = serde_json::to_string(&coordinates).expect("a string serializes");
    for (local, coordinates) in [
        ("Subscription:a-n12-geoq-array", coordinates.as_str()),
        ("Subscription:a-n12-geoq-string", encoded.as_str()),
    ] {
        let id = scope.id(local);
        let body = format!(
            r#"{{"id":"{id}","type":"Subscription","entities":[{{"type":"{}"}}],"geoQ":{{"geometry":"Polygon","coordinates":{coordinates},"georel":"intersects"}},"notification":{{"endpoint":{{"uri":"http://127.0.0.1:9/notify"}}}}}}"#,
            scope.entity_type("Probe")
        );

        let refused = send_json(scope.client(), Method::POST, "/ngsi-ld/v1/subscriptions", body).await;

        assert_problem(
            &refused,
            StatusCode::PAYLOAD_TOO_LARGE,
            REQUEST_ENTITY_TOO_LARGE,
            &[&GEOMETRY_POSITIONS.to_string(), "geoQ"],
        );
        let absent = scope
            .client()
            .raw(Method::GET, &format!("/ngsi-ld/v1/subscriptions/{id}"))
            .send()
            .await
            .expect("retrieve request");
        assert_eq!(absent.status, StatusCode::NOT_FOUND, "{:?}", absent.body);
    }
    scope.finish().await.expect("cleanup");
}

/// Clause 6.3.2: a registration's `location` past the position budget is
/// refused with 413, and no registration is created.
#[tokio::test]
async fn regression_a_n12_a_registration_location_over_the_budget_is_413() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Registration:a-n12-location");
    let body = format!(
        r#"{{"id":"{id}","type":"ContextSourceRegistration","information":[{{"entities":[{{"type":"{}"}}]}}],"endpoint":"http://127.0.0.1:9","location":{}}}"#,
        scope.entity_type("Probe"),
        polygon_text(&ring(GEOMETRY_POSITIONS + 1))
    );

    let refused = send_json(scope.client(), Method::POST, "/ngsi-ld/v1/csourceRegistrations", body).await;

    assert_problem(
        &refused,
        StatusCode::PAYLOAD_TOO_LARGE,
        REQUEST_ENTITY_TOO_LARGE,
        &[&GEOMETRY_POSITIONS.to_string(), "location"],
    );
    let absent = scope
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
        .send()
        .await
        .expect("retrieve request");
    assert_eq!(absent.status, StatusCode::NOT_FOUND, "{:?}", absent.body);
    scope.finish().await.expect("cleanup");
}

/// Clauses 5.8.6 and 5.2.12: a deletion's subscriptions are filtered on the
/// state it removed. One whose `q` the removed entity passes is notified once;
/// one whose `q` it fails is not -- which only a deletion that read the
/// removed state can tell apart.
#[tokio::test]
async fn regression_a_n12_a_deletion_with_a_matching_subscription_still_notifies() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("Probe");
    let id = scope.id("Probe:notified");
    let passing = scope.id("Subscription:a-n12-deletion-retired");
    let failing = scope.id("Subscription:a-n12-deletion-active");
    subscribe_to_deletions(scope.client(), &adapter, &passing, &entity_type, &["entityDeleted"], "state==\"retired\"").await;
    subscribe_to_deletions(scope.client(), &adapter, &failing, &entity_type, &["entityDeleted"], "state==\"active\"").await;

    let created = send_json(
        scope.client(),
        Method::POST,
        "/ngsi-ld/v1/entities",
        located_entity_in_state(&id, &entity_type, &polygon_text(&ring(10_000)), "retired"),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let deleted = scope.client().delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let notification = adapter.wait_for_subscription_notification(&passing, ARRIVAL).await;
    adapter.wait_for_no_subscription_notification(&passing, QUIET).await;
    adapter.wait_for_no_subscription_notification(&failing, QUIET).await;
    assert_eq!(notification.body["data"][0]["id"], id.as_str(), "{}", notification.body);
    assert!(notification.body["data"][0].get("deletedAt").is_some(), "{}", notification.body);
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup");
}

/// Table 5.2.12-1: `entityUpdated` includes `attributeDeleted`, so a
/// subscription listening for it alone is owed the deletion of the entity's
/// attributes, filtered on the state the deletion removed.
#[tokio::test]
async fn regression_a_n12_a_deletion_notifies_a_subscription_listening_for_entity_updated() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("Probe");
    let id = scope.id("Probe:updated-watcher");
    let subscription = scope.id("Subscription:a-n12-entity-updated");
    // Created before the subscription, so the only change it is owed is the
    // deletion's.
    let created = send_json(
        scope.client(),
        Method::POST,
        "/ngsi-ld/v1/entities",
        located_entity_in_state(&id, &entity_type, &polygon_text(&ring(10)), "retired"),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    subscribe_to_deletions(scope.client(), &adapter, &subscription, &entity_type, &["entityUpdated"], "state==\"retired\"").await;
    let deleted = scope.client().delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let notification = adapter.wait_for_subscription_notification(&subscription, ARRIVAL).await;
    assert_eq!(notification.body["data"][0]["id"], id.as_str(), "{}", notification.body);
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup");
}
