//! Spatial (georel) query integration tests (NGSI-LD 4.10.2).
//!
//! Naming convention: `spatial_<op>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod simplified_geojson;

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};

async fn create(client: &NgsiLdClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn query(client: &NgsiLdClient, query_str: &str) -> (StatusCode, HeaderMap, Value) {
    let path = format!("/ngsi-ld/v1/entities?{query_str}");
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", &jsonld_link_header(TEST_SUITE_CONTEXT))
        .send()
        .await
        .expect("query failed");
    (response.status, response.headers, response.body)
}

async fn query_compound(client: &NgsiLdClient, query_str: &str) -> (StatusCode, Value) {
    let path = format!("/ngsi-ld/v1/entities?{query_str}");
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", &jsonld_link_header(COMPOUND_CONTEXT))
        .send()
        .await
        .expect("query failed");
    (response.status, response.body)
}

async fn patch_location_point(client: &NgsiLdClient, id: &str, lon: f64, lat: f64) {
    let fragment = json!({
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [lon, lat]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.update_entity_attrs(id, &fragment).await.expect("patch_location failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

fn building_point(id: &str, lon: f64, lat: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [lon, lat]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn building_polygon(id: &str, coords: Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": coords}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn building_custom_geoproperty(id: &str, attr: &str, lon: f64, lat: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        attr: {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [lon, lat]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

// --- within (NGSI-LD 4.10.2) ---

#[tokio::test]
async fn spatial_within_point_in_polygon_returns_entity() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let inside = scope.id("Building:within_in");
    let outside = scope.id("Building:within_out");
    create(&client, &building_point(&inside, 13.4, 52.52)).await;
    create(&client, &building_point(&outside, 20.0, 40.0)).await;

    let coords = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";
    let filter = id_filter(&[&inside, &outside]);
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=within&geometry=Polygon&coordinates={coords}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(got.contains(&inside));
    assert!(!got.contains(&outside));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_within_no_match_returns_empty() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:within_none");
    create(&client, &building_point(&id, 0.0, 0.0)).await;

    let coords = "%5B%5B%5B10.0,10.0%5D,%5B11.0,10.0%5D,%5B11.0,11.0%5D,%5B10.0,11.0%5D,%5B10.0,10.0%5D%5D%5D";
    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

// --- contains ---

#[tokio::test]
async fn spatial_contains_polygon_contains_point_returns_entity() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let hall = scope.id("Building:contains_hall");
    let coords = json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]);
    create(&client, &building_polygon(&hall, coords)).await;

    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=contains&geometry=Point&coordinates=%5B1.0,1.0%5D",
            urlencoding::encode(&hall)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&hall));
    scope.cleanup().await.expect("cleanup failed");
}

// --- intersects ---

#[tokio::test]
async fn spatial_intersects_overlapping_polygons_returns_entity() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:intersects");
    let coords = json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]);
    create(&client, &building_polygon(&id, coords)).await;

    let ref_coords = "%5B%5B%5B1.0,1.0%5D,%5B3.0,1.0%5D,%5B3.0,3.0%5D,%5B1.0,3.0%5D,%5B1.0,1.0%5D%5D%5D";
    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=intersects&geometry=Polygon&coordinates={ref_coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

// --- disjoint ---

#[tokio::test]
async fn spatial_disjoint_excludes_overlapping() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let overlap = scope.id("Building:dj_over");
    let far = scope.id("Building:dj_far");
    create(&client, &building_point(&overlap, 0.5, 0.5)).await;
    create(&client, &building_point(&far, 10.0, 10.0)).await;

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let filter = id_filter(&[&overlap, &far]);
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=disjoint&geometry=Polygon&coordinates={coords}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(!got.contains(&overlap));
    assert!(got.contains(&far));
    scope.cleanup().await.expect("cleanup failed");
}

// --- equals ---

#[tokio::test]
async fn spatial_equals_same_coords_matches() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:eq_match");
    create(&client, &building_point(&id, 13.3986, 52.5547)).await;

    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=equals&geometry=Point&coordinates=%5B13.3986,52.5547%5D",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_equals_different_coords_does_not_match() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:eq_nomatch");
    create(&client, &building_point(&id, 13.3986, 52.5547)).await;

    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=equals&geometry=Point&coordinates=%5B0.0,0.0%5D",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

// --- overlaps ---

#[tokio::test]
async fn spatial_overlaps_partial_intersection_matches() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:overlaps");
    let coords = json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]);
    create(&client, &building_polygon(&id, coords)).await;

    let ref_coords = "%5B%5B%5B1.0,1.0%5D,%5B3.0,1.0%5D,%5B3.0,3.0%5D,%5B1.0,3.0%5D,%5B1.0,1.0%5D%5D%5D";
    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=overlaps&geometry=Polygon&coordinates={ref_coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

// --- near (maxDistance / minDistance) ---

#[tokio::test]
async fn spatial_near_max_distance_includes_within_radius() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let near_id = scope.id("Building:near_in");
    let far_id = scope.id("Building:near_out");
    create(&client, &building_point(&near_id, 13.5, 52.5547)).await;
    create(&client, &building_point(&far_id, 20.0, 52.5547)).await;

    let filter = id_filter(&[&near_id, &far_id]);
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(got.contains(&near_id));
    assert!(!got.contains(&far_id));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_near_min_distance_excludes_inside_inner_radius() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let inner = scope.id("Building:near_inner");
    let outer = scope.id("Building:near_outer");
    create(&client, &building_point(&inner, 13.389830, 52.534473)).await;
    create(&client, &building_point(&outer, 13.464_0, 52.534473)).await;

    let filter = id_filter(&[&inner, &outer]);
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=near;minDistance==1000&geometry=Point&coordinates=%5B13.388930,52.534473%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(!got.contains(&inner));
    assert!(got.contains(&outer));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_near_band_min_and_max() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let too_close = scope.id("Building:band_close");
    let in_band = scope.id("Building:band_in");
    let too_far = scope.id("Building:band_far");
    create(&client, &building_point(&too_close, 13.389830, 52.534473)).await;
    create(&client, &building_point(&in_band, 13.433, 52.534473)).await;
    create(&client, &building_point(&too_far, 14.0, 52.534473)).await;

    let filter = id_filter(&[&too_close, &in_band, &too_far]);
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=near;minDistance==500;maxDistance==10000&geometry=Point&coordinates=%5B13.388930,52.534473%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(!got.contains(&too_close));
    assert!(got.contains(&in_band));
    assert!(!got.contains(&too_far));
    scope.cleanup().await.expect("cleanup failed");
}

// --- Combined filters ---

#[tokio::test]
async fn spatial_georel_with_type_filter_restricts_by_type() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let building_id = scope.id("Building:type_flt");
    let vehicle_id = scope.id("Vehicle:type_flt");
    create(&client, &building_point(&building_id, 0.5, 0.5)).await;
    create(
        &client,
        &json!({
            "id": vehicle_id,
            "type": "Vehicle",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.5, 0.5]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let filter = id_filter(&[&building_id, &vehicle_id]);
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=within&geometry=Polygon&coordinates={coords}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(got.contains(&building_id));
    assert!(!got.contains(&vehicle_id));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_georel_with_id_filter_intersection() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let a = scope.id("Building:id_flt_A");
    let b = scope.id("Building:id_flt_B");
    let c_id = scope.id("Building:id_flt_C");
    for id in [&a, &b, &c_id] {
        create(&client, &building_point(id, 0.5, 0.5)).await;
    }

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let filter = id_filter(&[&a, &b]);
    let (status, _, body) = query(
        &client,
        &format!("id={filter}&type=Building&georel=within&geometry=Polygon&coordinates={coords}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let got = ids_of(&body);
    assert!(got.contains(&a));
    assert!(got.contains(&b));
    assert!(!got.contains(&c_id));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_georel_custom_geoproperty_routes_to_named_attribute() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:custom_geo");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
        "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.5, 52.5]}},
        "@context": [COMPOUND_CONTEXT]
    });
    create(&client, &entity).await;

    let coords = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";
    let (status, body) = query_compound(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}&geoproperty=location",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_georel_unknown_custom_geoproperty_returns_empty() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:unknown_geo");
    create(&client, &building_custom_geoproperty(&id, "location", 13.4, 52.5)).await;

    let coords = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";
    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}&geoproperty=doesNotExist",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

// --- Invalidation — updates & deletes ---

#[tokio::test]
async fn spatial_georel_updated_entity_moves_between_partitions() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:upd");
    create(&client, &building_point(&id, 0.5, 0.5)).await;

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let (_, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert!(ids_of(&body).contains(&id));

    patch_location_point(&client, &id, 10.0, 10.0).await;

    let (_, _, body2) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert!(!ids_of(&body2).contains(&id));

    let coords2 = "%5B%5B%5B9.0,9.0%5D,%5B11.0,9.0%5D,%5B11.0,11.0%5D,%5B9.0,11.0%5D,%5B9.0,9.0%5D%5D%5D";
    let (_, _, body3) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords2}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert!(ids_of(&body3).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_georel_deleted_entity_is_excluded() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:del");
    create(&client, &building_point(&id, 0.5, 0.5)).await;
    let response = client.delete_entity(&id).await.expect("delete failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}

// --- Count & pagination with georel ---

#[tokio::test]
async fn spatial_georel_count_header_is_populated() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let mut ids = Vec::new();
    for i in 0..3 {
        let id = scope.id(&format!("Building:count_{i}"));
        create(&client, &building_point(&id, 13.388930 + (i as f64) * 0.001, 52.534473)).await;
        ids.push(id);
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let (status, headers, _) = query(
        &client,
        &format!("type=Building&id={filter}&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D&count=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let count = headers
        .get("ngsild-results-count")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .expect("ngsild-results-count present");
    assert!(count >= 3, "expected >=3 matches, got {count}");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_georel_limit_caps_result_set() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let mut ids = Vec::new();
    for i in 0..5 {
        let id = scope.id(&format!("Building:lim_{i}"));
        create(&client, &building_point(&id, 13.388930 + (i as f64) * 0.001, 52.534473)).await;
        ids.push(id);
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let (status, _, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D&limit=2"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().map_or(0, Vec::len), 2);
    scope.cleanup().await.expect("cleanup failed");
}

// --- Error handling ---

#[tokio::test]
async fn spatial_georel_invalid_georel_returns_400() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let (status, _, _) = query(&client, "type=Building&georel=nonsense&geometry=Point&coordinates=%5B0.0,0.0%5D").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn spatial_georel_missing_geometry_returns_400() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let (status, _, _) = query(&client, "type=Building&georel=within&coordinates=%5B0.0,0.0%5D").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn spatial_georel_near_without_distance_returns_400() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let (status, _, _) = query(&client, "type=Building&georel=near&geometry=Point&coordinates=%5B0.0,0.0%5D").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- Concurrency / consistency ---

#[tokio::test]
async fn spatial_georel_rapid_updates_are_reflected() {
    let scope = TestScope::new("spatial");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:rapid");
    create(&client, &building_point(&id, 0.5, 0.5)).await;

    for i in 0..5 {
        let lon = 10.0 + (i as f64) * 0.1;
        patch_location_point(&client, &id, lon, 10.0).await;
    }

    let coords = "%5B%5B%5B10.0,9.5%5D,%5B10.5,9.5%5D,%5B10.5,10.5%5D,%5B10.0,10.5%5D,%5B10.0,9.5%5D%5D%5D";
    let (status, _, body) = query(
        &client,
        &format!(
            "type=Building&id={}&georel=within&geometry=Polygon&coordinates={coords}",
            urlencoding::encode(&id)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids_of(&body).contains(&id));
    scope.cleanup().await.expect("cleanup failed");
}
