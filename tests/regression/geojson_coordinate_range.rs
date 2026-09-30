//! A `GeoJSON` position off the WGS 84 axes is refused wherever a client sends one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7 makes a `GeoProperty` value an IETF RFC 7946
//! geometry, clause 4.7.2 accepts one only when it represents "a valid Geometry",
//! and clause 4.10 requires a reference geometry's coordinates "as mandated by
//! IETF RFC 7946 [8], section 3.1.1". RFC 7946 section 4 places every position
//! in WGS 84 longitude and latitude, in decimal degrees: longitude from -180 to
//! 180, latitude pole to pole. The broker accepted any finite ordinate, so a
//! position off the axes was stored and then read three different ways: the
//! planar georels took it as written, `PostGIS` geography silently wrapped or
//! reflected it for `near`
//! (`POINT(-1 100)` became `POINT(-1 80)`), and the in-memory distance turned a
//! latitude past a pole into a wrong or NaN distance. A geoquery with such a
//! reference answered `200 []`.
//!
//! Every client-facing surface now answers `BadRequestData` (clause 5.5.4, and
//! clause 4.10's syntactic validity for geoqueries) whose detail names the
//! refused ordinate, its value and where in `coordinates` the position sits.
//! Both ends of each axis are positions, and altitude has no bound. A Context
//! Source answering with such a geometry has that one entity dropped with a
//! clause 6.3.17 warning, the rest of its answer kept.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.2 names for input the operation cannot accept.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The resource a forwarded query lands on at the Context Source.
const FORWARDED_QUERY_PATH: &str = "/ngsi-ld/v1/entities";

/// A `GeoJSON` `Point` at `coordinates`.
fn point(coordinates: &Value) -> Value {
    json!({"type": "Point", "coordinates": coordinates})
}

/// A normalized `GeoProperty` holding `geometry`.
fn geo_property(geometry: &Value) -> Value {
    json!({"type": "GeoProperty", "value": geometry})
}

/// An entity of `entity_type` whose `location` is `geometry`.
fn located(id: &str, entity_type: &str, geometry: &Value) -> Value {
    json!({"id": id, "type": entity_type, "location": geo_property(geometry), "@context": [TEST_SUITE_CONTEXT]})
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

/// Assert `response` refuses a position as `BadRequestData` whose detail holds
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

/// The URL-encoded `coordinates` query parameter for `coordinates`.
fn coordinates_parameter(coordinates: &str) -> String {
    format!("coordinates={}", urlencoding::encode(coordinates))
}

/// Clause 4.7 with RFC 7946 section 4: an entity whose `location` or
/// `observationSpace` holds a position off either axis is refused, naming the
/// ordinate, its value and the vertex, and is not created.
#[tokio::test]
async fn regression_coordinate_range_an_entity_with_a_position_off_the_axes_is_refused_and_not_created() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("RangeProbe");

    let cases = [
        (
            located(&scope.id("RangeProbe:both-axes"), &entity_type, &point(&json!([1000, 2000]))),
            vec!["longitude 1000 is outside [-180, 180]", "(at coordinates)"],
        ),
        (
            located(&scope.id("RangeProbe:past-pole"), &entity_type, &point(&json!([0, 90.000_001]))),
            vec!["latitude 90.000001 is outside [-90, 90]"],
        ),
        (
            json!({
                "id": scope.id("RangeProbe:observed-vertex"),
                "type": entity_type,
                "observationSpace": geo_property(&json!({"type": "Polygon", "coordinates": [[[0, 0], [10, 95], [10, 0], [0, 0]]]})),
                "@context": [TEST_SUITE_CONTEXT]
            }),
            vec!["observationSpace", "latitude 95 is outside [-90, 90]", "(at coordinates[0][1])"],
        ),
    ];
    let mut outcomes = Vec::new();
    for (entity, details) in cases {
        let id = entity["id"].as_str().expect("an id").to_owned();
        let created = send(client, Method::POST, "entities", &entity).await;
        let retrieved = read(client, &format!("entities/{}", urlencoding::encode(&id))).await.status;
        outcomes.push((id, created, retrieved, details));
    }
    scope.finish().await.expect("cleanup failed");

    for (id, created, retrieved, details) in outcomes {
        assert_refused(&created, &id, &details);
        assert_eq!(retrieved, StatusCode::NOT_FOUND, "{id} was not created");
    }
}

/// Both ends of each CRS84 axis are positions, and altitude, height above or
/// below the ellipsoid, has no bound: all are stored and read back as sent.
#[tokio::test]
async fn regression_coordinate_range_the_axis_ends_and_any_altitude_are_stored_as_sent() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("RangeProbe:axis-ends");
    let entity = json!({
        "id": id,
        "type": scope.entity_type("RangeProbe"),
        "location": geo_property(&point(&json!([180, 90]))),
        "observationSpace": geo_property(&point(&json!([-180, -90]))),
        "operationSpace": geo_property(&point(&json!([14.5, 46.05, -11_000]))),
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = send(client, Method::POST, "entities", &entity).await;
    let retrieved = read(client, &format!("entities/{}", urlencoding::encode(&id))).await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let body = retrieved.json_value().expect("the entity");
    // Compared as numbers: the rendering may spell an integral ordinate `180.0`.
    for (attribute, coordinates) in [
        ("location", vec![180.0, 90.0]),
        ("observationSpace", vec![-180.0, -90.0]),
        ("operationSpace", vec![14.5, 46.05, -11_000.0]),
    ] {
        let returned: Vec<f64> = body[attribute]["value"]["coordinates"]
            .as_array()
            .unwrap_or_else(|| panic!("{attribute} carries coordinates: {body}"))
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        assert_eq!(returned, coordinates, "{attribute}: {body}");
    }
}

/// Clause 5.6.2: an update moving `location` off the axes is refused and the
/// stored value stays as it was.
#[tokio::test]
async fn regression_coordinate_range_an_update_off_the_axes_is_refused_and_leaves_the_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("RangeProbe:updated");
    let created = send(
        client,
        Method::POST,
        "entities",
        &located(&id, &scope.entity_type("RangeProbe"), &point(&json!([14.5, 46.05]))),
    )
    .await;
    let patch = json!({"location": geo_property(&point(&json!([0, -91]))), "@context": [TEST_SUITE_CONTEXT]});
    let patched = send(client, Method::PATCH, &format!("entities/{}/attrs", urlencoding::encode(&id)), &patch).await;
    let retrieved = read(client, &format!("entities/{}", urlencoding::encode(&id))).await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_refused(&patched, "update attributes", &["latitude -91 is outside [-90, 90]"]);
    let body = retrieved.json_value().expect("the entity");
    assert_eq!(body["location"]["value"]["coordinates"], json!([14.5, 46.05]), "{body}");
}

/// Clauses 5.6.7.4 and 5.6.8.4: in a batch, the entity with a position off the
/// axes is the one refused, reported against its id; its neighbour is written.
#[tokio::test]
async fn regression_coordinate_range_a_batch_refuses_only_the_entity_off_the_axes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("RangeProbe");
    let mut outcomes = Vec::new();
    for operation in ["create", "upsert"] {
        let valid = scope.id(&format!("RangeProbe:batch-{operation}-valid"));
        let invalid = scope.id(&format!("RangeProbe:batch-{operation}-invalid"));
        let batch = json!([
            located(&valid, &entity_type, &point(&json!([14.5, 46.05]))),
            located(&invalid, &entity_type, &point(&json!([0, 91])))
        ]);
        let answered = send(client, Method::POST, &format!("entityOperations/{operation}"), &batch).await;
        let valid_status = read(client, &format!("entities/{}", urlencoding::encode(&valid))).await.status;
        let invalid_status = read(client, &format!("entities/{}", urlencoding::encode(&invalid))).await.status;
        outcomes.push((operation, valid, invalid, answered, valid_status, invalid_status));
    }
    scope.finish().await.expect("cleanup failed");

    for (operation, valid, invalid, answered, valid_status, invalid_status) in outcomes {
        assert_eq!(answered.status, StatusCode::MULTI_STATUS, "{operation}: {:?}", answered.body);
        let result = answered.json_value().expect("a BatchOperationResult");
        assert_eq!(result["success"], json!([valid]), "{operation}: {result}");
        assert_eq!(result["errors"][0]["entityId"], json!(invalid), "{operation}: {result}");
        assert_eq!(result["errors"][0]["error"]["type"], json!(BAD_REQUEST_DATA), "{operation}: {result}");
        let detail = result["errors"][0]["error"]["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("latitude 91 is outside [-90, 90]"), "{operation}: {detail}");
        assert_eq!(valid_status, StatusCode::OK, "{operation}: the valid neighbour is written");
        assert_eq!(invalid_status, StatusCode::NOT_FOUND, "{operation}: the refused entity is not");
    }
}

/// Clause 5.6.11: a Temporal Evolution one of whose instances is off the axes
/// is refused before anything is stored, with the same detail as the Core API.
#[tokio::test]
async fn regression_coordinate_range_a_temporal_evolution_off_the_axes_is_refused_with_its_detail() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("RangeProbe:temporal");
    let evolution = json!({
        "id": id,
        "type": scope.entity_type("RangeProbe"),
        "location": [
            {"type": "GeoProperty", "value": point(&json!([14.5, 46.05])), "observedAt": "2025-01-01T00:00:00Z"},
            {"type": "GeoProperty", "value": point(&json!([181, 0])), "observedAt": "2025-01-01T00:01:00Z"}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = send(client, Method::POST, "temporal/entities", &evolution).await;
    let retrieved = read(client, &format!("temporal/entities/{}", urlencoding::encode(&id))).await.status;
    scope.finish().await.expect("cleanup failed");

    assert_refused(&created, "temporal create", &["longitude 181 is outside [-180, 180]"]);
    assert_eq!(retrieved, StatusCode::NOT_FOUND, "the refused evolution was not stored");
}

/// Clause 4.10: a reference geometry off the axes makes the geoquery
/// syntactically invalid, on every query surface, instead of matching nothing.
#[tokio::test]
async fn regression_coordinate_range_a_reference_geometry_off_the_axes_is_refused_on_every_query() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("RangeProbe");
    let near = format!("georel=near;maxDistance==10&geometry=Point&{}", coordinates_parameter("[1000,2000]"));
    let within = format!("georel=within&geometry=Polygon&{}", coordinates_parameter("[[[0,0],[10,0],[10,91],[0,0]]]"));
    let window = "timerel=after&timeAt=2020-01-01T00:00:00Z";
    let reads = [
        (format!("entities?type={entity_type}&{near}"), vec!["longitude 1000 is outside [-180, 180]"]),
        (
            format!("entities?type={entity_type}&{within}"),
            vec!["latitude 91 is outside [-90, 90]", "(at coordinates[0][2])"],
        ),
        (
            format!("temporal/entities?type={entity_type}&{window}&{near}"),
            vec!["longitude 1000 is outside [-180, 180]"],
        ),
        (
            format!(
                "entities?type={entity_type}&orderBy=location;dist-asc&orderFrom={}",
                urlencoding::encode("[0,91]")
            ),
            vec!["latitude 91 is outside [-90, 90]"],
        ),
        (
            format!("csourceRegistrations?type={entity_type}&{near}"),
            vec!["longitude 1000 is outside [-180, 180]"],
        ),
    ];
    let mut outcomes = Vec::new();
    for (path, details) in reads {
        outcomes.push((read(client, &path).await, path, details));
    }
    let post_query = json!({
        "type": "Query",
        "entities": [{"type": entity_type}],
        "geoQ": {"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": [0, 91]},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let posted = send(client, Method::POST, "entityOperations/query", &post_query).await;
    scope.finish().await.expect("cleanup failed");

    for (response, path, details) in outcomes {
        assert_refused(&response, &path, &details);
    }
    assert_refused(&posted, "POST entityOperations/query", &["latitude 91 is outside [-90, 90]"]);
}

/// Clauses 5.8.1 and 5.11.2: a `geoQ` whose reference position is off the axes
/// is refused on both kinds of subscription.
#[tokio::test]
async fn regression_coordinate_range_a_subscription_geoq_off_the_axes_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let mut outcomes = Vec::new();
    for resource in ["subscriptions", "csourceSubscriptions"] {
        let subscription = json!({
            "id": scope.id(&format!("Subscription:range-{resource}")),
            "type": "Subscription",
            "entities": [{"type": scope.entity_type("RangeProbe")}],
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [-1, 100]},
            "notification": {"endpoint": {"uri": "http://127.0.0.1:65535/notify", "accept": "application/json"}},
            "@context": [TEST_SUITE_CONTEXT]
        });
        outcomes.push((resource, send(client, Method::POST, resource, &subscription).await));
    }
    scope.finish().await.expect("cleanup failed");

    for (resource, response) in outcomes {
        assert_refused(&response, resource, &["latitude 100 is outside [-90, 90]"]);
    }
}

/// Table 5.2.13-1 gives `georel`, `geometry` and `coordinates` cardinality 1.
/// A `geoQ` missing one is refused on create (clause 5.8.1.4) and on update
/// (clauses 5.8.2.4 and 5.5.4), where clause 5.5.8 would otherwise store the
/// incomplete `geoQ` whole; such a `geoQ` used to be admitted and then matched
/// every entity.
#[tokio::test]
async fn regression_coordinate_range_an_incomplete_subscription_geoq_is_refused_on_create_and_update() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:incomplete-geoq");
    let subscription = |geo_q: Value| {
        json!({
            "id": id,
            "type": "Subscription",
            "entities": [{"type": scope.entity_type("RangeProbe")}],
            "geoQ": geo_q,
            "notification": {"endpoint": {"uri": "http://127.0.0.1:65535/notify", "accept": "application/json"}},
            "@context": [TEST_SUITE_CONTEXT]
        })
    };
    let incomplete = send(
        client,
        Method::POST,
        "subscriptions",
        &subscription(json!({"georel": "near;maxDistance==2000"})),
    )
    .await;
    let complete = send(
        client,
        Method::POST,
        "subscriptions",
        &subscription(json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [14.5, 46.05]})),
    )
    .await;
    let update = json!({"geoQ": {"georel": "within", "geometry": "Point"}, "@context": [TEST_SUITE_CONTEXT]});
    let updated = send(client, Method::PATCH, &format!("subscriptions/{}", urlencoding::encode(&id)), &update).await;
    let retrieved = read(client, &format!("subscriptions/{}", urlencoding::encode(&id))).await;
    let _deleted = client.raw(Method::DELETE, &format!("subscriptions/{}", urlencoding::encode(&id))).send().await;
    scope.finish().await.expect("cleanup failed");

    assert_refused(&incomplete, "create with georel only", &["'geometry'", "Table 5.2.13-1"]);
    assert_eq!(complete.status, StatusCode::CREATED, "{:?}", complete.body);
    assert_refused(&updated, "update without coordinates", &["'coordinates'"]);
    let body = retrieved.json_value().expect("the subscription");
    assert_eq!(body["geoQ"]["coordinates"], json!([14.5, 46.05]), "the refused update changed nothing: {body}");
}

/// Clauses 5.9.2 and 5.9.3: a registration's `location` or `operationSpace`
/// off the axes is refused on create and on update, and a refused update
/// leaves the registration as it was.
#[tokio::test]
async fn regression_coordinate_range_a_registration_geometry_off_the_axes_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("RangeProbe");
    let registration = |id: &str, location: &Value| {
        json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": "http://127.0.0.1:65535",
            "information": [{"entities": [{"type": entity_type}]}],
            "location": location,
            "@context": [TEST_SUITE_CONTEXT]
        })
    };
    let refused_id = scope.id("registration:range-refused");
    let kept_id = scope.id("registration:range-kept");
    let refused = send(
        client,
        Method::POST,
        "csourceRegistrations",
        &registration(&refused_id, &point(&json!([0, 91]))),
    )
    .await;
    let kept = send(
        client,
        Method::POST,
        "csourceRegistrations",
        &registration(&kept_id, &point(&json!([14.5, 46.05]))),
    )
    .await;
    let update = json!({"operationSpace": point(&json!([-181, 0])), "@context": [TEST_SUITE_CONTEXT]});
    let updated = send(
        client,
        Method::PATCH,
        &format!("csourceRegistrations/{}", urlencoding::encode(&kept_id)),
        &update,
    )
    .await;
    let retrieved = read(client, &format!("csourceRegistrations/{}", urlencoding::encode(&kept_id))).await;
    let _deleted = client
        .raw(Method::DELETE, &format!("csourceRegistrations/{}", urlencoding::encode(&kept_id)))
        .send()
        .await;
    scope.finish().await.expect("cleanup failed");

    assert_refused(&refused, "create registration", &["latitude 91 is outside [-90, 90]"]);
    assert_eq!(kept.status, StatusCode::CREATED, "{:?}", kept.body);
    assert_refused(&updated, "update registration", &["longitude -181 is outside [-180, 180]"]);
    let body = retrieved.json_value().expect("the registration");
    assert!(
        body.get("operationSpace").is_none_or(Value::is_null),
        "the refused update changed nothing: {body}"
    );
}

/// Clause 5.7.2.4 with clause 6.3.17: a Context Source answering with one
/// entity off the axes has that entity dropped with a warning, and the rest of
/// its answer is merged as usual.
#[tokio::test]
async fn regression_coordinate_range_a_source_entity_off_the_axes_is_dropped_with_a_warning() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("RangeSourceProbe");
    let valid = scope.id("RangeSourceProbe:valid");
    let invalid = scope.id("RangeSourceProbe:invalid");
    let source = ContextSourceAdapter::start().await;
    let answer = json!([
        {"id": valid, "type": entity_type, "location": geo_property(&point(&json!([14.5, 46.05]))), "@context": TEST_SUITE_CONTEXT.as_str()},
        {"id": invalid, "type": entity_type, "location": geo_property(&point(&json!([14.5, 146.05]))), "@context": TEST_SUITE_CONTEXT.as_str()}
    ]);
    source.set_stub_with_body(Method::GET, FORWARDED_QUERY_PATH, 200, &answer).await;
    let (registered, queried) = {
        let client = scope.client();
        let registration = json!({
            "id": scope.id("registration:range-source"),
            "type": "ContextSourceRegistration",
            "endpoint": source.url("/"),
            "mode": "inclusive",
            "operations": ["federationOps"],
            "information": [{"entities": [{"type": entity_type}]}],
            "@context": [TEST_SUITE_CONTEXT]
        });
        let registered = client.create_registration(&registration).await.expect("create registration").status;
        let queried = client
            .query_entities(&format!("type={entity_type}"), "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
            .await
            .expect("query entities");
        (registered, queried)
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(registered, StatusCode::CREATED);
    assert_eq!(queried.status, StatusCode::OK, "{:?}", queried.body);
    let ids: Vec<String> = queried
        .json_value()
        .expect("a JSON page")
        .as_array()
        .expect("a query answers an array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(ids, vec![valid], "only the source's valid entity is merged");
    let warnings: Vec<&str> = queried
        .headers
        .get_all("ngsild-warning")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.starts_with("111") && warning.contains("latitude 146.05 is outside [-90, 90]")),
        "the dropped entity is reported as a clause 6.3.17 revalidation failure: {warnings:?}"
    );
}
