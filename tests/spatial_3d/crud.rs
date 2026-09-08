//! Round-tripping of three-dimensional `GeoJSON` geometries through entity
//! create, retrieve, batch create and attribute patch (NGSI-LD 4.7.1,
//! RFC 7946 3.1.1).
//!
//! The broker must echo the exact coordinate arity it was given: an altitude
//! that was supplied is returned, and one that was not supplied is not
//! invented.

use crate::fixtures::{
    assert_coordinates_eq,
    building_point_2d,
    building_point_3d,
    building_polygon_3d,
    building_with_location,
    create,
    geoproperty_coordinates,
    get_entity,
    ids_of,
    location_coordinates,
    p0,
    p500,
    query,
};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{PgPool, Row};

/// Reads `ST_Zmflag` for an entity's hoisted `location` column: 0 for an XY
/// geometry, 2 for XYZ. Migration 0005 constrains the column to those two.
async fn location_zmflag(pool: &PgPool, id: &str) -> i32 {
    // ST_Zmflag returns smallint; the cast keeps the binding type unambiguous.
    let row = sqlx::query("SELECT ST_Zmflag(s.location)::int AS zmflag FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE e.id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("entity row must exist for storage assertion");
    row.try_get::<i32, _>("zmflag").expect("ST_Zmflag must be readable as an integer")
}

#[tokio::test]
async fn crud_point_3d_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:point3d");

    create(client, &building_point_3d(&id, 13.4, 52.52, 300.5)).await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &json!([13.4, 52.52, 300.5]));

    // Migration 0032 stores XYZ verbatim rather than flattening to XY.
    assert_eq!(location_zmflag(scope.database_pool(), &id).await, 2);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_linestring_3d_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:linestring3d");
    let coordinates = json!([[13.4, 52.52, 100.0], [13.5, 52.53, 200.0], [13.6, 52.54, 300.0]]);

    create(client, &building_with_location(&id, json!({"type": "LineString", "coordinates": coordinates}))).await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_polygon_3d_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:polygon3d");
    let coordinates = json!([[
        [13.0, 52.0, 10.0],
        [14.0, 52.0, 20.0],
        [14.0, 53.0, 30.0],
        [13.0, 53.0, 40.0],
        [13.0, 52.0, 10.0]
    ]]);

    create(client, &building_polygon_3d(&id, coordinates.clone())).await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_multipoint_3d_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:multipoint3d");
    let coordinates = json!([[13.4, 52.52, 10.0], [13.5, 52.53, 20.0]]);

    create(client, &building_with_location(&id, json!({"type": "MultiPoint", "coordinates": coordinates}))).await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_multilinestring_3d_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:multilinestring3d");
    let coordinates = json!([[[13.0, 52.0, 1.0], [13.1, 52.1, 2.0]], [[13.5, 52.5, 3.0], [13.6, 52.6, 4.0]]]);

    create(
        client,
        &building_with_location(&id, json!({"type": "MultiLineString", "coordinates": coordinates})),
    )
    .await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_multipolygon_3d_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:multipolygon3d");
    let coordinates = json!([
        [[[13.0, 52.0, 5.0], [13.2, 52.0, 5.0], [13.2, 52.2, 5.0], [13.0, 52.2, 5.0], [13.0, 52.0, 5.0]]],
        [[[14.0, 53.0, 6.0], [14.2, 53.0, 6.0], [14.2, 53.2, 6.0], [14.0, 53.2, 6.0], [14.0, 53.0, 6.0]]]
    ]);

    create(
        client,
        &building_with_location(&id, json!({"type": "MultiPolygon", "coordinates": coordinates})),
    )
    .await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_mixed_arity_linestring_preserved_verbatim() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:mixedarity");
    // RFC 7946 3.1.1 constrains each position independently, so a geometry may
    // mix two- and three-ordinate positions and the broker must not normalise.
    let coordinates = json!([[13.4, 52.52], [13.5, 52.53, 250.0]]);

    create(client, &building_with_location(&id, json!({"type": "LineString", "coordinates": coordinates}))).await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_entity_with_2d_location_and_3d_custom_geoproperty() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:mixedattrs");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 120.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });

    create(client, &entity).await;

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &json!([13.4, 52.52]));
    assert_coordinates_eq(geoproperty_coordinates(&body, "observationPoint"), &json!([13.4, 52.52, 120.0]));

    // The hoisted location column stays XY even though a sibling GeoProperty is XYZ.
    assert_eq!(location_zmflag(scope.database_pool(), &id).await, 0);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_batch_create_3d_entities_round_trip() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let ground = scope.id("Building:batch_ground");
    let elevated = scope.id("Building:batch_elevated");

    let batch = json!([building_point_3d(&ground, 13.4, 52.52, 0.0), building_point_3d(&elevated, 13.4, 52.52, 500.0)]);
    let response = client.batch_create(&batch).await.expect("batch create failed");
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::MULTI_STATUS,
        "batch create of two valid 3D entities must succeed, got {}",
        response.status
    );

    let (status, ground_body) = get_entity(client, &ground).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&ground_body), &p0());

    let (status, elevated_body) = get_entity(client, &elevated).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&elevated_body), &p500());

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_patch_attrs_upgrades_2d_location_to_3d() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:upgrade");

    create(client, &building_point_2d(&id, 13.4, 52.52)).await;

    let fragment = json!({
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 400.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let patch = client.update_entity_attrs(&id, &fragment).await.expect("patch failed");
    assert_eq!(patch.status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &json!([13.4, 52.52, 400.0]));
    assert_eq!(location_zmflag(scope.database_pool(), &id).await, 2);

    // A 2D near query on the same footprint still matches (NGSI-LD 4.10).
    let (status, _, matched) = query(
        client,
        &format!(
            "type=Building&id={}&georel=near;maxDistance==100&geometry=Point&coordinates=%5B13.4,52.52%5D",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&matched).contains(&id), "3D entity must match a 2D near query");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn crud_patch_attrs_downgrades_3d_location_to_2d() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:downgrade");

    create(client, &building_point_3d(&id, 13.4, 52.52, 400.0)).await;

    let fragment = json!({
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let patch = client.update_entity_attrs(&id, &fragment).await.expect("patch failed");
    assert_eq!(patch.status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    let coordinates = location_coordinates(&body);
    assert_coordinates_eq(coordinates, &json!([13.4, 52.52]));
    assert_eq!(coordinates.as_array().map(Vec::len), Some(2), "altitude must not be reinstated");
    assert_eq!(location_zmflag(scope.database_pool(), &id).await, 0);

    scope.finish().await.expect("cleanup failed");
}

/// Guards the assumption that the mixed-arity fixtures above are not silently
/// rewritten into a uniform arity by a later read path.
#[tokio::test]
async fn crud_mixed_arity_survives_no_op_attribute_patch() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:mixedarity_patch");
    let coordinates = json!([[13.4, 52.52], [13.5, 52.53, 250.0]]);

    create(client, &building_with_location(&id, json!({"type": "LineString", "coordinates": coordinates}))).await;

    let fragment = json!({
        "category": {"type": "Property", "value": "tower"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let patch = client.append_entity_attrs(&id, &fragment).await.expect("append failed");
    assert!(
        patch.status == StatusCode::NO_CONTENT || patch.status == StatusCode::MULTI_STATUS,
        "attribute append must succeed, got {}",
        patch.status
    );

    let (status, body) = get_entity(client, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_coordinates_eq(location_coordinates(&body), &coordinates);

    scope.finish().await.expect("cleanup failed");
}
