//! The temporal join's window semantics (`temporal_linked_retrieval`).
//!
//! A temporal answer is a slice of history, so a joined Linked Entity is
//! rendered over exactly the window the roots were rendered over, and only the
//! Relationship instances that fall inside that window name a candidate at all.
//! `join=flat` puts the Linked Entities beside the roots (clause 6.4.3.2);
//! `join=inline` nests each one under the instances that name it.
//!
//! Clauses 5.7.3.4 and 5.7.4.4 do not admit Linked Entity retrieval on the
//! temporal operations, so these cases run only against a broker started with
//! `ANDROMEDA__EXTENSIONS__TEMPORAL_LINKED_RETRIEVAL=enabled`.

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::{COMPOUND_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The window every case below reads over.
const WINDOW: &str = "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T12:10:00Z";

/// A Vehicle whose speed was recorded twice inside the window.
fn vehicle_with_speed_history(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 30, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 40, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// A Vehicle parked in `inside` throughout the window, and in `outside` at an
/// instant well beyond it.
fn vehicle_parked_across_the_window(id: &str, inside: &str, outside: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 10, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 20, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "isParkedIn": [
            { "type": "Relationship", "object": inside, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Relationship", "object": inside, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Relationship", "object": outside, "observedAt": "2020-09-01T13:00:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Create one Temporal Entity, asserting the broker accepted it.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.create_temporal_entity(entity).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one request under the suite context.
async fn send(client: &TestClient, method: Method, path: &str, body: Option<&Value>) -> TestResponse {
    let request = client
        .raw(method, path)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    let request = match body {
        Some(body) => request.header("Content-Type", "application/json").json(body),
        None => request,
    };
    request.send().await.expect("request")
}

/// Query the Vehicles of one run with `parameters`, asserting a 200.
async fn query_ok(scope: &TestEnvironment, parameters: &str) -> Value {
    let path = format!("temporal/entities?{}", scope.scope_query(&format!("type=Vehicle&{WINDOW}&{parameters}")));
    let response = send(scope.client(), Method::GET, &path, None).await;
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a 200 carries a JSON body")
}

/// The `id` member of every entity in an array body, in response order.
fn ids_from_array(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a joined temporal response is an array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

/// The one array member carrying `id`.
fn member_with_id<'a>(body: &'a Value, id: &str) -> &'a Value {
    let member = body
        .as_array()
        .expect("a joined temporal response is an array")
        .iter()
        .find(|entity| entity["id"].as_str() == Some(id));
    assert!(member.is_some(), "no array member with id {id}: {body}");
    member.expect("the array member was just asserted to be present")
}

/// The `observedAt` of every instance a temporal attribute member carries.
fn observed_instants(member: &Value) -> Vec<String> {
    member
        .as_array()
        .expect("a temporal attribute renders as an array of instances")
        .iter()
        .filter_map(|instance| instance["observedAt"].as_str().map(str::to_owned))
        .collect()
}

/// Clause 6.4.3.2: a flat join appends the Linked Entities after the roots, and
/// the Temporal API renders them the way it renders everything else, as the
/// history each attribute had over the requested window.
#[tokio::test]
async fn temporal_join_flat_appends_linked_temporal_entity() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let linked = scope.id("Vehicle:tjoin-flat-B");
    let root = scope.id("Vehicle:tjoin-flat-A");
    let elsewhere = scope.id("Vehicle:tjoin-flat-C");
    create(scope.client(), &vehicle_with_speed_history(&linked)).await;
    create(scope.client(), &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await;

    let body = query_ok(&scope, &format!("id={root}&join=flat&joinLevel=1")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&root), "the root must be present: {ids:?}");
    assert!(ids.contains(&linked), "the Linked Entity must accompany the root: {ids:?}");

    let joined = member_with_id(&body, &linked);
    assert_eq!(
        observed_instants(&joined["speed"]),
        vec!["2020-08-01T12:03:00Z".to_owned(), "2020-08-01T12:05:00Z".to_owned()],
        "the Linked Entity renders as temporal history over the same window: {joined}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// An inline join nests instead of appending: every in-window instance of the
/// Relationship carries the Linked Entity's own windowed history under
/// `entity`, and the collection still holds exactly the roots it started with.
#[tokio::test]
async fn temporal_join_inline_nests_under_instances() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let linked = scope.id("Vehicle:tjoin-inline-B");
    let root = scope.id("Vehicle:tjoin-inline-A");
    let elsewhere = scope.id("Vehicle:tjoin-inline-C");
    create(scope.client(), &vehicle_with_speed_history(&linked)).await;
    create(scope.client(), &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await;

    let body = query_ok(&scope, &format!("id={root}&join=inline&joinLevel=1")).await;
    assert_eq!(ids_from_array(&body), vec![root.clone()], "an inline join appends nothing");

    let instances = member_with_id(&body, &root)["isParkedIn"]
        .as_array()
        .expect("the Relationship renders as an array of instances");
    assert_eq!(instances.len(), 2, "only the in-window instances are rendered: {instances:?}");
    for instance in instances {
        assert_eq!(
            instance["entity"]["id"].as_str(),
            Some(linked.as_str()),
            "every instance nests the Linked Entity: {instance}"
        );
        assert_eq!(
            observed_instants(&instance["entity"]["speed"]),
            vec!["2020-08-01T12:03:00Z".to_owned(), "2020-08-01T12:05:00Z".to_owned()],
            "the nested entity carries its own windowed history: {instance}"
        );
    }
    scope.finish().await.expect("cleanup failed");
}

/// A Relationship instance outside the window is not rendered, so the entity it
/// names is never a traversal candidate, even when that entity has history of
/// its own inside the window.
#[tokio::test]
async fn temporal_join_window_limits_candidates() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let linked = scope.id("Vehicle:tjoin-window-B");
    let root = scope.id("Vehicle:tjoin-window-A");
    let elsewhere = scope.id("Vehicle:tjoin-window-C");
    create(scope.client(), &vehicle_with_speed_history(&linked)).await;
    create(scope.client(), &vehicle_with_speed_history(&elsewhere)).await;
    create(scope.client(), &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await;

    let body = query_ok(&scope, &format!("id={root}&join=flat&joinLevel=1")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&linked), "the in-window Linked Entity must be present: {ids:?}");
    assert!(!ids.contains(&elsewhere), "an entity named only outside the window must not be joined: {ids:?}");
    scope.finish().await.expect("cleanup failed");
}

/// The POST query binding carries `join` and `joinLevel` in its body exactly as
/// the GET carries them in its query string, and reaches the same traversal.
#[tokio::test]
async fn temporal_post_query_join() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let linked = scope.id("Vehicle:tjoin-post-B");
    let root = scope.id("Vehicle:tjoin-post-A");
    let elsewhere = scope.id("Vehicle:tjoin-post-C");
    create(scope.client(), &vehicle_with_speed_history(&linked)).await;
    create(scope.client(), &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await;

    let query = json!({
        "type": "Query",
        "entities": [{ "id": root, "type": "Vehicle" }],
        "join": "flat",
        "joinLevel": 1,
        "temporalQ": {
            "timerel": "between",
            "timeAt": "2020-08-01T12:00:00Z",
            "endTimeAt": "2020-08-01T12:10:00Z"
        }
    });
    let response = send(scope.client(), Method::POST, "temporal/entityOperations/query", Some(&query)).await;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json_value().expect("a 200 carries a JSON body");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&root), "the root must be present: {ids:?}");
    assert!(ids.contains(&linked), "the Linked Entity must accompany the root: {ids:?}");
    assert!(!ids.contains(&elsewhere), "an entity named only outside the window must not be joined: {ids:?}");
    scope.finish().await.expect("cleanup failed");
}

/// With the extension on, the temporal query follows the clause 5.7.2.4 row of
/// Query Entities: a `q` hop needs a join that retrieves.
#[tokio::test]
async fn temporal_query_rejects_a_traversing_filter_without_a_retrieving_join() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let filter = urlencoding::encode("isParkedIn{speed>5}");
    let path = format!("temporal/entities?{}", scope.scope_query(&format!("type=Vehicle&{WINDOW}&q={filter}")));
    let response = send(scope.client(), Method::GET, &path, None).await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
    scope.finish().await.expect("cleanup failed");
}

/// The projection half of the same row: a nested `pick` term describes a
/// Linked Entity, so it is refused without a retrieving join and honoured once
/// one is requested.
#[tokio::test]
async fn temporal_query_bounds_a_nested_projection_by_the_requested_join() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let linked = scope.id("Vehicle:tjoin-pick-B");
    let root = scope.id("Vehicle:tjoin-pick-A");
    let elsewhere = scope.id("Vehicle:tjoin-pick-C");
    create(scope.client(), &vehicle_with_speed_history(&linked)).await;
    create(scope.client(), &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await;

    let pick = urlencoding::encode("isParkedIn{speed}");
    let path = format!(
        "temporal/entities?{}",
        scope.scope_query(&format!("id={root}&type=Vehicle&{WINDOW}&pick={pick}"))
    );
    let refused = send(scope.client(), Method::GET, &path, None).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);

    query_ok(&scope, &format!("id={root}&pick={pick}&join=flat&joinLevel=1")).await;
    scope.finish().await.expect("cleanup failed");
}
