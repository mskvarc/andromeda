//! Rejection of coordinate arities the standards do not allow
//! (NGSI-LD 5.5.4, 6.3.2; RFC 7946 3.1.1).
//!
//! RFC 7946 3.1.1 permits exactly two or three ordinates per position. Anything
//! else — four ordinates, a lone ordinate, a non-numeric ordinate — is invalid
//! GeoJSON and the broker answers 400 `BadRequestData` on every surface that
//! accepts a geometry: entity bodies, query parameters, POST query bodies,
//! subscriptions, temporal queries, ordering references and registrations.

use crate::fixtures::{assert_coordinates_eq, building_point_3d, create, encode_json, get_entity, location_coordinates, query};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Asserts a body is an NGSI-LD `BadRequestData` ProblemDetails document.
fn assert_bad_request_data(body: &Value) {
    assert_eq!(
        body["type"].as_str().expect("ProblemDetails must carry a type"),
        error::BAD_REQUEST_DATA.as_str(),
        "expected BadRequestData, got body {body}"
    );
    assert!(body.get("title").and_then(Value::as_str).is_some(), "ProblemDetails must carry a title");
}

/// Posts an entity expected to be rejected and returns the ProblemDetails body.
async fn create_expecting_bad_request(client: &TestClient, entity: &Value) -> Value {
    let response = client.post_entity(entity).send().await.expect("create request failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "entity create must be rejected");
    let body = response.json_value().expect("error body must be valid JSON");
    assert_bad_request_data(&body);
    body
}

/// An entity whose `location` carries the supplied raw geometry.
fn entity_with_geometry(id: &str, geometry: Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": geometry},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

#[tokio::test]
async fn validation_four_ordinate_point_location_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:four_ordinates");

    let body = create_expecting_bad_request(
        client,
        &entity_with_geometry(&id, json!({"type": "Point", "coordinates": [13.4, 52.52, 300.0, 7.0]})),
    )
    .await;

    let detail = body["detail"].as_str().expect("BadRequestData must explain the rejection");
    assert!(detail.contains("4 ordinates"), "detail must name the offending arity, got {detail}");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_four_ordinate_polygon_vertex_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:polygon_four");
    // A single bad vertex invalidates the whole geometry.
    let coordinates = json!([[
        [13.0, 52.0, 10.0],
        [14.0, 52.0, 10.0, 4.0],
        [14.0, 53.0, 10.0],
        [13.0, 53.0, 10.0],
        [13.0, 52.0, 10.0]
    ]]);

    create_expecting_bad_request(client, &entity_with_geometry(&id, json!({"type": "Polygon", "coordinates": coordinates}))).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_single_ordinate_point_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:one_ordinate");

    create_expecting_bad_request(client, &entity_with_geometry(&id, json!({"type": "Point", "coordinates": [13.4]}))).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_non_numeric_longitude_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:string_lon");

    create_expecting_bad_request(client, &entity_with_geometry(&id, json!({"type": "Point", "coordinates": ["13.4", 52.52]}))).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_non_numeric_altitude_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:string_alt");

    create_expecting_bad_request(
        client,
        &entity_with_geometry(&id, json!({"type": "Point", "coordinates": [13.4, 52.52, "high"]})),
    )
    .await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_geometrycollection_location_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometrycollection");
    // NGSI-LD 4.6.3 and 4.7.1: every RFC 7946 geometry is allowed as a
    // GeoProperty value *except* GeometryCollection.
    let geometry = json!({
        "type": "GeometryCollection",
        "geometries": [
            {"type": "Point", "coordinates": [13.4, 52.52, 300.0]},
            {"type": "LineString", "coordinates": [[13.4, 52.52, 0.0], [13.5, 52.53, 100.0]]}
        ]
    });

    create_expecting_bad_request(client, &entity_with_geometry(&id, geometry)).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_four_ordinate_custom_geoproperty_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:custom_four");
    // The location is valid; only the sibling GeoProperty is malformed.
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 10.0]}},
        "observationPoint": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4, 52.52, 10.0, 1.0]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });

    create_expecting_bad_request(client, &entity).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_patch_to_four_ordinates_is_rejected_and_leaves_value_intact() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:patch_four");

    create(client, &building_point_3d(&id, 13.4, 52.52, 300.0)).await;

    let fragment = json!({
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4, 52.52, 300.0, 9.0]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let patch = client.update_entity_attrs(&id, &fragment).await.expect("patch request failed");
    assert_eq!(patch.status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&patch.json_value().expect("error body must be valid JSON"));

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &json!([13.4, 52.52, 300.0]));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_batch_create_reports_per_entity_coordinate_errors() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let good = scope.id("Building:batch_good");
    let bad = scope.id("Building:batch_bad");

    let batch = json!([
        building_point_3d(&good, 13.4, 52.52, 300.0),
        entity_with_geometry(&bad, json!({"type": "Point", "coordinates": [13.4, 52.52, 300.0, 7.0]}))
    ]);
    let response = client.batch_create(&batch).await.expect("batch create failed");
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "a partially valid batch reports 207");

    let body = response.json_value().expect("batch result must be valid JSON");
    let reported_success = body["success"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .any(|entity_id| entity_id == good.as_str());
    assert!(reported_success, "the valid entity must be reported as a success");

    let errors = body["errors"].as_array().map(Vec::as_slice).unwrap_or_default();
    let bad_error = errors
        .iter()
        .find(|entry| entry.get("entityId").and_then(Value::as_str) == Some(bad.as_str()))
        .expect("the 4-ordinate entity must be reported as an error");
    assert_eq!(bad_error.pointer("/error/type").and_then(Value::as_str), Some(error::BAD_REQUEST_DATA.as_str()));

    let (status, _) = get_entity(client, &good).await;
    assert_eq!(status, StatusCode::OK, "the valid entity must have been created");

    let missing = client
        .get_entity(&bad, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve request failed");
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "the rejected entity must not exist");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_query_four_ordinate_coordinates_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();

    let coordinates = encode_json(&json!([13.4, 52.52, 300.0, 7.0]));
    let (status, _, body) = query(
        client,
        &format!("type=Building&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&body);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_post_query_four_ordinate_geoq_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();

    let query_body = json!({
        "type": "Query",
        "entities": [{"type": "Building"}],
        "geoQ": {
            "georel": "near;maxDistance==100",
            "geometry": "Point",
            "coordinates": [13.4, 52.52, 300.0, 7.0]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client
        .batch_query(&query_body, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("POST query failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&response.json_value().expect("error body must be valid JSON"));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_query_three_ordinate_coordinates_is_accepted() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();

    let coordinates = encode_json(&json!([13.4, 52.52, 300.0]));
    let (status, _, _) = query(
        client,
        &format!("type=Building&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a 3D reference geometry is valid GeoJSON and must be accepted");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_subscription_four_ordinate_geoq_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let subscription_id = scope.id("Subscription:four_ordinates");

    // NGSI-LD 5.8.1: the subscription geoQ is validated at creation time.
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "geoQ": {
            "georel": "near;maxDistance==100",
            "geometry": "Point",
            "coordinates": [13.4, 52.52, 300.0, 7.0]
        },
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9999/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&response.json_value().expect("error body must be valid JSON"));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_temporal_query_four_ordinate_coordinates_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();

    let coordinates = encode_json(&json!([13.4, 52.52, 300.0, 7.0]));
    let query_string = format!("type=Building&timerel=after&timeAt=2020-01-01T00:00:00Z&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}");
    let response = client
        .query_temporal_entities(&query_string, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal query failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&response.json_value().expect("error body must be valid JSON"));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_order_from_four_ordinates_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();

    let order_from = encode_json(&json!([13.4, 52.52, 0.0, 9.0]));
    let (status, _, body) = query(client, &format!("type=Building&orderBy=location;dist-asc&orderFrom={order_from}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&body);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn validation_registration_four_ordinate_location_returns_bad_request() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:four_ordinates");

    let registration = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://127.0.0.1:9999",
        "location": {"type": "Point", "coordinates": [13.4, 52.52, 300.0, 7.0]},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_registration(&registration).await.expect("create registration failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_bad_request_data(&response.json_value().expect("error body must be valid JSON"));

    scope.finish().await.expect("cleanup failed");
}
