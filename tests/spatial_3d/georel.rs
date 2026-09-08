//! Spatial predicates evaluate on the 2D footprint (NGSI-LD 4.10,
//! OGC 06-103r4 6.1.15).
//!
//! NGSI-LD 4.10 defines its georel values by reference to the OGC simple
//! feature access predicates, which are specified on the XY projection. An
//! altitude therefore never affects whether a georel matches: a 3D entity and a
//! 2D one at the same longitude/latitude behave identically, and a reference
//! geometry may carry an altitude that is simply ignored.

use crate::fixtures::{
    building_point_2d,
    building_point_3d,
    building_polygon_3d,
    create,
    encode_json,
    id_filter,
    ids_of,
    p_2d,
    p0,
    pillar_ring_2d,
    pillar_ring_3d,
    query,
};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn georel_within_2d_polygon_reference_matches_3d_point() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:within_3d_point");

    create(client, &building_point_3d(&id, 13.4, 52.52, 500.0)).await;

    let coordinates = encode_json(&pillar_ring_2d());
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coordinates}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "a 3D point inside a 2D reference footprint must match");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_within_3d_polygon_reference_matches_2d_point() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:within_2d_point");

    create(client, &building_point_2d(&id, 13.4, 52.52)).await;

    let coordinates = encode_json(&pillar_ring_3d(10.0));
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coordinates}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "a 2D point inside a 3D reference footprint must match");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_within_3d_vs_3d_ignores_altitude() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:within_high");

    // The point sits 9 km above a ring drawn at 10 m; the footprints coincide.
    create(client, &building_point_3d(&id, 13.4, 52.52, 9000.0)).await;

    let coordinates = encode_json(&pillar_ring_3d(10.0));
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coordinates}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "altitude must not affect within");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_contains_3d_polygon_contains_reference_point_at_other_altitude() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:contains_3d");

    create(client, &building_polygon_3d(&id, pillar_ring_3d(10.0))).await;

    let coordinates = encode_json(&json!([13.4, 52.52, 8000.0]));
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=contains&geometry=Point&coordinates={coordinates}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "contains must ignore the altitude of both operands");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_intersects_3d_geometries_intersect_on_footprint() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:intersects_3d");

    // Entity ring at altitude 0, reference ring at altitude 5000: disjoint in
    // space, overlapping on the footprint, so they intersect per OGC semantics.
    create(client, &building_polygon_3d(&id, pillar_ring_3d(0.0))).await;

    let reference = json!([[
        [13.5, 52.5, 5000.0],
        [14.5, 52.5, 5000.0],
        [14.5, 53.5, 5000.0],
        [13.5, 53.5, 5000.0],
        [13.5, 52.5, 5000.0]
    ]]);
    let coordinates = encode_json(&reference);
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=intersects&geometry=Polygon&coordinates={coordinates}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "footprint overlap must count as an intersection");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_disjoint_3d_same_footprint_not_disjoint() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let overlapping = scope.id("Building:disjoint_over");
    let far = scope.id("Building:disjoint_far");

    create(client, &building_point_3d(&overlapping, 13.4, 52.52, 5000.0)).await;
    create(client, &building_point_3d(&far, 30.0, 10.0, 5000.0)).await;

    let coordinates = encode_json(&pillar_ring_3d(0.0));
    let filter = id_filter(&[&overlapping, &far]);
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&georel=disjoint&geometry=Polygon&coordinates={coordinates}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let matched = ids_of(&body);
    assert!(!matched.contains(&overlapping), "a shared footprint is not disjoint, whatever the altitudes");
    assert!(matched.contains(&far), "a genuinely separate footprint is disjoint");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_equals_matches_across_dimensionality() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let flat = scope.id("Building:equals_2d");
    let elevated = scope.id("Building:equals_3d");

    create(client, &building_point_2d(&flat, 13.4, 52.52)).await;
    create(client, &building_point_3d(&elevated, 13.4, 52.52, 500.0)).await;

    let filter = id_filter(&[&flat, &elevated]);
    for reference in [p_2d(), p0()] {
        let coordinates = encode_json(&reference);
        let (status, _, body) = query(
            client,
            &format!("type=Building&id={filter}&georel=equals&geometry=Point&coordinates={coordinates}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let matched = ids_of(&body);
        assert!(matched.contains(&flat), "2D entity must equal reference {reference}");
        assert!(matched.contains(&elevated), "3D entity must equal reference {reference}");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_overlaps_3d_polygons_overlap_on_footprint() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let overlapping = scope.id("Building:overlaps_3d");
    let far = scope.id("Building:overlaps_far");

    create(client, &building_polygon_3d(&overlapping, pillar_ring_3d(0.0))).await;
    let far_ring = json!([[[30.0, 10.0, 0.0], [31.0, 10.0, 0.0], [31.0, 11.0, 0.0], [30.0, 11.0, 0.0], [30.0, 10.0, 0.0]]]);
    create(client, &building_polygon_3d(&far, far_ring)).await;

    // Partially overlapping ring at a different altitude.
    let reference = json!([[
        [13.5, 52.5, 900.0],
        [14.5, 52.5, 900.0],
        [14.5, 53.5, 900.0],
        [13.5, 53.5, 900.0],
        [13.5, 52.5, 900.0]
    ]]);
    let coordinates = encode_json(&reference);
    let filter = id_filter(&[&overlapping, &far]);
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&georel=overlaps&geometry=Polygon&coordinates={coordinates}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let matched = ids_of(&body);
    assert!(matched.contains(&overlapping), "footprint overlap must match regardless of altitude");
    assert!(!matched.contains(&far), "a distant footprint must not overlap");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn georel_near_ignores_altitude() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let ground = scope.id("Building:near_ground");
    let elevated = scope.id("Building:near_elevated");

    create(client, &building_point_3d(&ground, 13.4, 52.52, 0.0)).await;
    create(client, &building_point_3d(&elevated, 13.4, 52.52, 500.0)).await;

    let filter = id_filter(&[&ground, &elevated]);
    // A 500 m vertical separation must not push the entity outside a 100 m
    // radius, because near is a horizontal (geodesic) distance.
    for reference in [p0(), p_2d()] {
        let coordinates = encode_json(&reference);
        let (status, _, body) = query(
            client,
            &format!("type=Building&id={filter}&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let matched = ids_of(&body);
        assert!(matched.contains(&ground), "ground entity must match reference {reference}");
        assert!(matched.contains(&elevated), "elevated entity must match reference {reference}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.10 lets `geoproperty` select which `GeoProperty` a georel evaluates
/// against, and the selected one is matched on its 2D footprint like any other,
/// so a stored Z ordinate does not affect the outcome.
#[tokio::test]
async fn georel_near_3d_custom_geoproperty() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:near_custom");

    // The location is far away; only the named GeoProperty is near the
    // reference, so a match proves the query routed to observationPoint.
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [30.0, 10.0]}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 800.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let coordinates = encode_json(&p0());
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}&geoproperty=observationPoint",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "a 3D custom GeoProperty must match on horizontal distance alone");

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.10 names `observationSpace` among the `GeoProperties` a geo-query may
/// target (clause 4.7.1), and its example 1 targets exactly that one. It has a
/// dedicated storage column, so it is the case most likely to be routed to the
/// wrong place.
#[tokio::test]
async fn georel_near_observation_space_matches() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:near_observation_space");

    // The location is far away, so a match proves the query routed to
    // observationSpace rather than falling back to the default.
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [30.0, 10.0]}},
        "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let coordinates = encode_json(&p0());
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}&geoproperty=observationSpace",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "observationSpace must be matched when it is the named GeoProperty");

    scope.finish().await.expect("cleanup failed");
}

/// A custom `GeoProperty` carrying a plain 2D geometry matches on the same terms:
/// clause 4.10 draws no distinction between the `GeoProperties` it defines and
/// ones a Context defines.
#[tokio::test]
async fn georel_near_2d_custom_geoproperty_matches() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:near_custom_2d");

    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [30.0, 10.0]}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let coordinates = encode_json(&p0());
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}&geoproperty=observationPoint",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id), "a 2D custom GeoProperty must match on its own geometry");

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.10: "Entities which do not convey the target `GeoProperty` of the
/// query shall be considered as non-matching." That is an empty result set and
/// not an error, whether the named attribute is absent from the entity or
/// present but not a `GeoProperty`.
#[tokio::test]
async fn georel_near_geoproperty_the_entity_does_not_convey_matches_nothing() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:no_such_geoproperty");

    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "category": {"type": "Property", "value": "office"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let coordinates = encode_json(&p0());
    // `category` is an ordinary Property and `absentSpace` is not present at
    // all; neither is a GeoProperty this entity conveys.
    for geoproperty in ["category", "absentSpace"] {
        let (status, _, body) = query(
            client,
            &format!(
                "type=Building&id={}&georel=near;maxDistance==100000&geometry=Point&coordinates={coordinates}&geoproperty={geoproperty}",
                urlencoding::encode(&id)
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "geoproperty={geoproperty} must not be an error");
        assert!(
            !ids_of(&body).contains(&id),
            "geoproperty={geoproperty} names no GeoProperty the entity conveys, so it must not match"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.10: where a `GeoProperty` has several `datasetId` instances, it is
/// enough that one of them meets the restriction.
///
/// The default instance is placed far from the reference and only a tagged one
/// is near it, so a match proves the filter is an existential over the instance
/// rows rather than a test of the default instance alone.
#[tokio::test]
async fn georel_near_custom_geoproperty_matches_on_a_non_default_dataset_instance() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:near_multi_dataset");

    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [30.0, 10.0]}},
        "observationPoint": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [30.0, 10.0]}},
            {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.4, 52.52, 800.0]},
                "datasetId": "urn:ngsi-ld:Dataset:sensor-a"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let coordinates = encode_json(&p0());
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}&geoproperty=observationPoint",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ids_of(&body).contains(&id),
        "one instance meeting the restriction is enough, whichever datasetId carries it"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The converse of the previous test: an entity none of whose instances meets
/// the restriction must not match, so the existential is not vacuously true.
#[tokio::test]
async fn georel_near_custom_geoproperty_matches_no_entity_whose_every_instance_is_distant() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:near_multi_dataset_miss");

    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "observationPoint": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [30.0, 10.0]}},
            {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [31.0, 11.0]},
                "datasetId": "urn:ngsi-ld:Dataset:sensor-a"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let coordinates = encode_json(&p0());
    let (status, _, body) = query(
        client,
        &format!(
            "type=Building&id={}&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}&geoproperty=observationPoint",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !ids_of(&body).contains(&id),
        "no instance of the named GeoProperty is near the reference, and the default location must not stand in for it"
    );

    scope.finish().await.expect("cleanup failed");
}
