//! A Polygon ring or `LineString` that RFC 7946 does not admit is refused wherever a client sends one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7.2 accepts a `GeoProperty` value only when it is "a valid Geometry", and clause 4.10 requires a reference geometry's coordinates "as mandated by IETF RFC 7946 [8], section 3.1.1". RFC 7946 section 3.1.4 gives a `LineString` two or more positions, and section 3.1.6 makes every Polygon ring "a closed `LineString` with four or more positions" whose first and last positions "MUST contain identical values". Storing an open ring would hand clients a geometry no other `GeoJSON` consumer is required to accept.
//!
//! Every client-facing surface answers `BadRequestData` (clause 5.5.4, and clause 4.10's syntactic validity for geoqueries) whose detail names the rule and where in `coordinates` the refused ring or line sits. Winding order is not checked: section 3.1.6 says parsers "SHOULD NOT reject Polygons that do not follow the right-hand rule".

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.2 names for input the operation cannot accept.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The detail of a ring whose last position does not repeat its first.
const OPEN_RING: &str = "GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first";

/// A closed square ring around the origin, drawn anticlockwise.
fn closed_square() -> Value {
    json!([[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]])
}

/// The same square left open.
fn open_square() -> Value {
    json!([[0, 0], [1, 0], [1, 1], [0, 1]])
}

/// A normalized `GeoProperty` holding `geometry`.
fn geo_property(geometry: &Value) -> Value {
    json!({"type": "GeoProperty", "value": geometry})
}

/// A JSON-LD request carrying `body` to `path`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, path)
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("request")
}

/// A read of `path` with the test suite context.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/ld+json")
        .header(
            "Link",
            format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"").as_str(),
        )
        .send()
        .await
        .expect("read request")
}

/// Assert `response` refuses a geometry as `BadRequestData` whose detail holds
/// every one of `details`.
fn assert_refused(response: &TestResponse, scenario: &str, details: &[&str]) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    for expected in details {
        assert!(detail.contains(expected), "{scenario}: the detail must name {expected:?}: {detail}");
    }
}

/// Integral ordinates are rendered as `1.0`; comparing as `f64` makes them equal to the `1` a fixture wrote.
fn as_f64(value: &Value) -> Value {
    match value {
        Value::Number(number) => number.as_f64().map_or(Value::Null, |ordinate| json!(ordinate)),
        Value::Array(items) => Value::Array(items.iter().map(as_f64).collect()),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Object(_) => value.clone(), // clone: the comparison owns its normalized copy
    }
}

/// RFC 7946 sections 3.1.4 and 3.1.6: an open ring, a ring of three positions, an open hole of a later polygon or a one-position line is refused, naming the rule and the ring or line, and nothing is created.
#[tokio::test]
async fn regression_ring_structure_an_entity_with_an_inadmissible_ring_or_line_is_refused_and_not_created() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("RingProbe");
    let cases = [
        (
            "open-ring",
            "location",
            json!({"type": "Polygon", "coordinates": [open_square()]}),
            vec![OPEN_RING, "(at coordinates[0])"],
        ),
        (
            "short-ring",
            "observationSpace",
            json!({"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [0, 0]]]}),
            vec![
                "GeoJSON linear ring has 3 of the four or more positions RFC 7946 section 3.1.6 requires",
                "(at coordinates[0])",
            ],
        ),
        (
            "open-hole",
            "operationSpace",
            json!({"type": "MultiPolygon", "coordinates": [
                [closed_square()],
                [[[10, 10], [20, 10], [20, 20], [10, 20], [10, 10]], [[12, 12], [14, 12], [14, 14], [12, 14]]]
            ]}),
            vec![OPEN_RING, "(at coordinates[1][1])"],
        ),
        (
            "short-line",
            "location",
            json!({"type": "LineString", "coordinates": [[0, 0]]}),
            vec![
                "GeoJSON LineString has 1 of the two or more positions RFC 7946 section 3.1.4 requires",
                "(at coordinates)",
            ],
        ),
    ];
    let mut outcomes = Vec::new();
    for (name, attribute, geometry, details) in cases {
        let id = scope.id(&format!("RingProbe:{name}"));
        let entity = json!({"id": id, "type": entity_type, attribute: geo_property(&geometry), "@context": [TEST_SUITE_CONTEXT]});
        let created = send(client, Method::POST, "entities", &entity).await;
        let retrieved = read(client, &format!("entities/{}", urlencoding::encode(&id))).await.status;
        outcomes.push((name, attribute, created, retrieved, details));
    }
    scope.finish().await.expect("cleanup failed");

    for (name, attribute, created, retrieved, details) in outcomes {
        assert_refused(&created, name, &details);
        assert_refused(&created, name, &[attribute]);
        assert_eq!(retrieved, StatusCode::NOT_FOUND, "{name} was not created");
    }
}

/// A closed ring is stored and read back exactly as sent, however it winds, as are a two-position line and a polygon with a hole.
#[tokio::test]
async fn regression_ring_structure_closed_rings_and_two_position_lines_are_stored_as_sent() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("RingProbe:admissible");
    let with_hole = json!({"type": "Polygon", "coordinates": [
        [[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]],
        [[1, 1], [1, 2], [2, 2], [2, 1], [1, 1]]
    ]});
    let clockwise = json!({"type": "Polygon", "coordinates": [[[0, 0], [0, 1], [1, 1], [1, 0], [0, 0]]]});
    let line = json!({"type": "LineString", "coordinates": [[0, 0], [1, 1, 50]]});
    let entity = json!({
        "id": id,
        "type": scope.entity_type("RingProbe"),
        "location": geo_property(&with_hole),
        "observationSpace": geo_property(&clockwise),
        "operationSpace": geo_property(&line),
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = send(client, Method::POST, "entities", &entity).await;
    let retrieved = read(client, &format!("entities/{}", urlencoding::encode(&id))).await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let body = retrieved.json_value().expect("the entity");
    for (attribute, geometry) in [("location", with_hole), ("observationSpace", clockwise), ("operationSpace", line)] {
        assert_eq!(
            as_f64(&body[attribute]["value"]["coordinates"]),
            as_f64(&geometry["coordinates"]),
            "{attribute}: {body}"
        );
    }
}

/// Clause 5.6.11: a Temporal Evolution whose instance holds an open ring is refused before anything is stored, with the same detail as the Core API.
#[tokio::test]
async fn regression_ring_structure_a_temporal_evolution_with_an_open_ring_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("RingProbe:temporal");
    let evolution = json!({
        "id": id,
        "type": scope.entity_type("RingProbe"),
        "location": [
            {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [closed_square()]}, "observedAt": "2025-01-01T00:00:00Z"},
            {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [open_square()]}, "observedAt": "2025-01-01T00:01:00Z"}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = send(client, Method::POST, "temporal/entities", &evolution).await;
    let retrieved = read(client, &format!("temporal/entities/{}", urlencoding::encode(&id))).await.status;
    scope.finish().await.expect("cleanup failed");

    assert_refused(&created, "temporal create", &[OPEN_RING]);
    assert_eq!(retrieved, StatusCode::NOT_FOUND, "the refused evolution was not stored");
}

/// Clause 4.10: an open reference ring makes the geoquery syntactically invalid on every query surface, and clauses 5.8.1 and 5.9.2 refuse it in a subscription's `geoQ` and a registration's `location` alike.
#[tokio::test]
async fn regression_ring_structure_an_open_reference_ring_is_refused_on_every_surface() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("RingProbe");
    let within = format!(
        "georel=within&geometry=Polygon&coordinates={}",
        urlencoding::encode(&json!([open_square()]).to_string())
    );
    let reads = [
        format!("entities?type={entity_type}&{within}"),
        format!("temporal/entities?type={entity_type}&timerel=after&timeAt=2020-01-01T00:00:00Z&{within}"),
        format!("csourceRegistrations?type={entity_type}&{within}"),
    ];
    let mut outcomes = Vec::new();
    for path in reads {
        outcomes.push((read(client, &path).await, path));
    }
    let open_geo_q = json!({"georel": "within", "geometry": "Polygon", "coordinates": [open_square()]});
    let post_query = json!({"type": "Query", "entities": [{"type": entity_type}], "geoQ": open_geo_q, "@context": [TEST_SUITE_CONTEXT]});
    outcomes.push((
        send(client, Method::POST, "entityOperations/query", &post_query).await,
        "POST entityOperations/query".to_owned(),
    ));
    let subscription = json!({
        "id": scope.id("Subscription:open-ring"),
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "geoQ": open_geo_q,
        "notification": {"endpoint": {"uri": "http://127.0.0.1:65535/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    outcomes.push((
        send(client, Method::POST, "subscriptions", &subscription).await,
        "POST subscriptions".to_owned(),
    ));
    let registration = json!({
        "id": scope.id("registration:open-ring"),
        "type": "ContextSourceRegistration",
        "endpoint": "http://127.0.0.1:65535",
        "information": [{"entities": [{"type": entity_type}]}],
        "location": {"type": "Polygon", "coordinates": [open_square()]},
        "@context": [TEST_SUITE_CONTEXT]
    });
    outcomes.push((
        send(client, Method::POST, "csourceRegistrations", &registration).await,
        "POST csourceRegistrations".to_owned(),
    ));
    scope.finish().await.expect("cleanup failed");

    for (response, scenario) in outcomes {
        assert_refused(&response, &scenario, &[OPEN_RING]);
    }
}
