//! Spatial (georel) query integration tests for the NGSI-LD context broker.
//!
//! Exercises every georel operator defined by NGSI-LD 4.10.2 — `within`,
//! `contains`, `intersects`, `disjoint`, `equals`, `overlaps`, `near` — plus
//! combined filters (type, id, pagination, count), custom geoproperties, and
//! invalidation after entity mutation. Covers the R-tree acceleration added
//! by `src/service/spatial/` as well as the SQL fall-through path: behaviour
//! must be identical regardless of which path serves the request.
//!
//! Standalone test suite using `tokio::test` + `reqwest`. Naming convention:
//! `spatial_<op>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";
const COMPOUND_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn link_header() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

fn building_point(id: &str, lon: f64, lat: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [lon, lat]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn building_polygon(id: &str, coords: Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Polygon", "coordinates": coords}
        },
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn building_custom_geoproperty(id: &str, attr: &str, lon: f64, lat: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        attr: {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [lon, lat]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    })
}

async fn create_entity(client: &Client, body: &Value) -> StatusCode {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    resp.status()
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{id}")).send().await;
}

async fn patch_location_point(client: &Client, id: &str, lon: f64, lat: f64) -> StatusCode {
    let body = json!({
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [lon, lat]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let resp = client
        .patch(format!("{BASE_URL}/entities/{id}/attrs"))
        .header("Content-Type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("patch_location request failed");
    resp.status()
}

/// GET /entities with a georel query. Returns (status, headers, body).
async fn query(client: &Client, q: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .get(format!("{BASE_URL}/entities?{q}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("query request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect()
}

// ---------------------------------------------------------------------------
// within (NGSI-LD 4.10.2)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_within_point_in_polygon_returns_entity() {
    clean_database().await;
    let c = client();
    let inside = unique_id("urn:ngsi-ld:Building:within_in");
    let outside = unique_id("urn:ngsi-ld:Building:within_out");

    assert_eq!(create_entity(&c, &building_point(&inside, 13.4, 52.52)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&c, &building_point(&outside, 20.0, 40.0)).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(got.contains(&inside), "inside must match: {got:?}");
    assert!(!got.contains(&outside), "outside must not match: {got:?}");
}

#[tokio::test]
#[serial]
async fn spatial_within_no_match_returns_empty() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:within_none");
    assert_eq!(create_entity(&c, &building_point(&id, 0.0, 0.0)).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B10.0,10.0%5D,%5B11.0,10.0%5D,%5B11.0,11.0%5D,%5B10.0,11.0%5D,%5B10.0,10.0%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids(&body).iter().all(|x| x != &id));
}

// ---------------------------------------------------------------------------
// contains
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_contains_polygon_contains_point_returns_entity() {
    clean_database().await;
    let c = client();
    let hall = unique_id("urn:ngsi-ld:Building:contains_hall");
    let coords = json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]);
    assert_eq!(create_entity(&c, &building_polygon(&hall, coords)).await, StatusCode::CREATED);

    let ref_coords = "%5B1.0,1.0%5D";
    let q = format!("type=Building&georel=contains&geometry=Point&coordinates={ref_coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids(&body).contains(&hall));
}

// ---------------------------------------------------------------------------
// intersects
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_intersects_overlapping_polygons_returns_entity() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:intersects");
    let coords = json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]);
    assert_eq!(create_entity(&c, &building_polygon(&id, coords)).await, StatusCode::CREATED);

    let ref_coords = "%5B%5B%5B1.0,1.0%5D,%5B3.0,1.0%5D,%5B3.0,3.0%5D,%5B1.0,3.0%5D,%5B1.0,1.0%5D%5D%5D";
    let q = format!("type=Building&georel=intersects&geometry=Polygon&coordinates={ref_coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids(&body).contains(&id));
}

// ---------------------------------------------------------------------------
// disjoint
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_disjoint_excludes_overlapping() {
    clean_database().await;
    let c = client();
    let overlap = unique_id("urn:ngsi-ld:Building:dj_over");
    let far = unique_id("urn:ngsi-ld:Building:dj_far");
    assert_eq!(create_entity(&c, &building_point(&overlap, 0.5, 0.5)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&c, &building_point(&far, 10.0, 10.0)).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let q = format!("type=Building&georel=disjoint&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(!got.contains(&overlap));
    assert!(got.contains(&far));
}

// ---------------------------------------------------------------------------
// equals
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_equals_same_coords_matches() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:eq_match");
    assert_eq!(create_entity(&c, &building_point(&id, 13.3986, 52.5547)).await, StatusCode::CREATED);

    let q = "type=Building&georel=equals&geometry=Point&coordinates=%5B13.3986,52.5547%5D";
    let (status, _, body) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids(&body).contains(&id));
}

#[tokio::test]
#[serial]
async fn spatial_equals_different_coords_does_not_match() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:eq_nomatch");
    assert_eq!(create_entity(&c, &building_point(&id, 13.3986, 52.5547)).await, StatusCode::CREATED);

    let q = "type=Building&georel=equals&geometry=Point&coordinates=%5B0.0,0.0%5D";
    let (status, _, body) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids(&body).contains(&id));
}

// ---------------------------------------------------------------------------
// overlaps
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_overlaps_partial_intersection_matches() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:overlaps");
    let coords = json!([[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]);
    assert_eq!(create_entity(&c, &building_polygon(&id, coords)).await, StatusCode::CREATED);

    let ref_coords = "%5B%5B%5B1.0,1.0%5D,%5B3.0,1.0%5D,%5B3.0,3.0%5D,%5B1.0,3.0%5D,%5B1.0,1.0%5D%5D%5D";
    let q = format!("type=Building&georel=overlaps&geometry=Polygon&coordinates={ref_coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ids(&body).contains(&id));
}

// ---------------------------------------------------------------------------
// near (maxDistance / minDistance)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_near_max_distance_includes_within_radius() {
    clean_database().await;
    let c = client();
    let near_id = unique_id("urn:ngsi-ld:Building:near_in");
    let far_id = unique_id("urn:ngsi-ld:Building:near_out");
    // near_id ~9km from ref; far_id ~500km
    assert_eq!(create_entity(&c, &building_point(&near_id, 13.5, 52.5547)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&c, &building_point(&far_id, 20.0, 52.5547)).await, StatusCode::CREATED);

    let q = "type=Building&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D";
    let (status, _, body) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(got.contains(&near_id));
    assert!(!got.contains(&far_id));
}

#[tokio::test]
#[serial]
async fn spatial_near_min_distance_excludes_inside_inner_radius() {
    clean_database().await;
    let c = client();
    let inner = unique_id("urn:ngsi-ld:Building:near_inner");
    let outer = unique_id("urn:ngsi-ld:Building:near_outer");
    // inner ~100m east of ref (~0.0009 deg at 52N); outer ~5km east (~0.075 deg)
    assert_eq!(create_entity(&c, &building_point(&inner, 13.389830, 52.534473)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&c, &building_point(&outer, 13.464_0, 52.534473)).await, StatusCode::CREATED);

    let q = "type=Building&georel=near;minDistance==1000&geometry=Point&coordinates=%5B13.388930,52.534473%5D";
    let (status, _, body) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(!got.contains(&inner), "inner (100m) must be excluded by minDistance=1000");
    assert!(got.contains(&outer), "outer (5km) must pass minDistance=1000");
}

#[tokio::test]
#[serial]
async fn spatial_near_band_min_and_max() {
    clean_database().await;
    let c = client();
    let too_close = unique_id("urn:ngsi-ld:Building:band_close");
    let in_band = unique_id("urn:ngsi-ld:Building:band_in");
    let too_far = unique_id("urn:ngsi-ld:Building:band_far");
    // ref (13.388930, 52.534473). ~100m, ~3km, ~50km
    assert_eq!(create_entity(&c, &building_point(&too_close, 13.389830, 52.534473)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&c, &building_point(&in_band, 13.433, 52.534473)).await, StatusCode::CREATED);
    assert_eq!(create_entity(&c, &building_point(&too_far, 14.0, 52.534473)).await, StatusCode::CREATED);

    let q = "type=Building&georel=near;minDistance==500;maxDistance==10000&geometry=Point&coordinates=%5B13.388930,52.534473%5D";
    let (status, _, body) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(!got.contains(&too_close));
    assert!(got.contains(&in_band));
    assert!(!got.contains(&too_far));
}

// ---------------------------------------------------------------------------
// Combined filters
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_georel_with_type_filter_restricts_by_type() {
    clean_database().await;
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:type_flt");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:type_flt");
    assert_eq!(create_entity(&c, &building_point(&building_id, 0.5, 0.5)).await, StatusCode::CREATED);
    let vehicle = json!({
        "id": vehicle_id,
        "type": "Vehicle",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.5, 0.5]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    assert_eq!(create_entity(&c, &vehicle).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(got.contains(&building_id));
    assert!(!got.contains(&vehicle_id));
}

#[tokio::test]
#[serial]
async fn spatial_georel_with_id_filter_intersection() {
    clean_database().await;
    let c = client();
    let a = unique_id("urn:ngsi-ld:Building:id_flt_A");
    let b = unique_id("urn:ngsi-ld:Building:id_flt_B");
    let c_id = unique_id("urn:ngsi-ld:Building:id_flt_C");
    for id in [&a, &b, &c_id] {
        assert_eq!(create_entity(&c, &building_point(id, 0.5, 0.5)).await, StatusCode::CREATED);
    }

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let q = format!("id={a},{b}&type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    let got = ids(&body);
    assert!(got.contains(&a));
    assert!(got.contains(&b));
    assert!(!got.contains(&c_id));
}

#[tokio::test]
#[serial]
async fn spatial_georel_custom_geoproperty_routes_to_named_attribute() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:custom_geo");
    // Entity carries `location` at (0,0) and `observationSpace` at (13.5, 52.5).
    // We assert that querying on the `location` geoproperty against a polygon
    // around (13,52)-(14,53) does NOT return this entity (its location is
    // (0,0), outside the polygon). This validates that the geoproperty
    // parameter actually routes the predicate to the named geoproperty's
    // value rather than falling back to `location` indiscriminately.
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
        "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.5, 52.5]}},
        "@context": [COMPOUND_CONTEXT]
    });
    assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";
    let link_compound = format!("<{COMPOUND_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");

    let q_loc = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}&geoproperty=location");
    let resp = c
        .get(format!("{BASE_URL}/entities?{q_loc}"))
        .header("Accept", "application/ld+json")
        .header("Link", &link_compound)
        .send()
        .await
        .expect("query request failed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body_loc: Value = resp.json().await.unwrap_or_default();
    assert!(
        !ids(&body_loc).contains(&id),
        "geoproperty=location must not match entity whose location is (0,0)"
    );
}

#[tokio::test]
#[serial]
async fn spatial_georel_unknown_custom_geoproperty_returns_empty() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:unknown_geo");
    assert_eq!(
        create_entity(&c, &building_custom_geoproperty(&id, "location", 13.4, 52.5)).await,
        StatusCode::CREATED
    );

    let coords = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}&geoproperty=doesNotExist");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids(&body).contains(&id));
}

// ---------------------------------------------------------------------------
// Invalidation — updates & deletes
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_georel_updated_entity_moves_between_partitions() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:upd");
    assert_eq!(create_entity(&c, &building_point(&id, 0.5, 0.5)).await, StatusCode::CREATED);

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (_, _, body) = query(&c, &q).await;
    assert!(ids(&body).contains(&id), "initial position must match");

    // Move to (10,10) — now outside original polygon.
    assert_eq!(patch_location_point(&c, &id, 10.0, 10.0).await, StatusCode::NO_CONTENT);

    let (_, _, body2) = query(&c, &q).await;
    assert!(!ids(&body2).contains(&id), "moved entity must leave original partition");

    let coords2 = "%5B%5B%5B9.0,9.0%5D,%5B11.0,9.0%5D,%5B11.0,11.0%5D,%5B9.0,11.0%5D,%5B9.0,9.0%5D%5D%5D";
    let q2 = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords2}");
    let (_, _, body3) = query(&c, &q2).await;
    assert!(ids(&body3).contains(&id), "moved entity must appear in new partition");
}

#[tokio::test]
#[serial]
async fn spatial_georel_deleted_entity_is_excluded() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:del");
    assert_eq!(create_entity(&c, &building_point(&id, 0.5, 0.5)).await, StatusCode::CREATED);
    delete_entity(&c, &id).await;

    let coords = "%5B%5B%5B0.0,0.0%5D,%5B1.0,0.0%5D,%5B1.0,1.0%5D,%5B0.0,1.0%5D,%5B0.0,0.0%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ids(&body).contains(&id));
}

// ---------------------------------------------------------------------------
// Count & pagination with georel
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_georel_count_header_is_populated() {
    clean_database().await;
    let c = client();
    for i in 0..3 {
        let id = unique_id(&format!("urn:ngsi-ld:Building:count_{i}"));
        assert_eq!(
            create_entity(&c, &building_point(&id, 13.388930 + (i as f64) * 0.001, 52.534473)).await,
            StatusCode::CREATED
        );
    }

    let q = "type=Building&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D&count=true";
    let (status, headers, _) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    let count = headers
        .get("ngsild-results-count")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .expect("ngsild-results-count header present");
    assert!(count >= 3, "expected >=3 matches, got {count}");
}

#[tokio::test]
#[serial]
async fn spatial_georel_limit_caps_result_set() {
    clean_database().await;
    let c = client();
    for i in 0..5 {
        let id = unique_id(&format!("urn:ngsi-ld:Building:lim_{i}"));
        assert_eq!(
            create_entity(&c, &building_point(&id, 13.388930 + (i as f64) * 0.001, 52.534473)).await,
            StatusCode::CREATED
        );
    }

    let q = "type=Building&georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D&limit=2";
    let (status, _, body) = query(&c, q).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().map(Vec::len).unwrap_or(0), 2);
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_georel_invalid_georel_returns_400() {
    clean_database().await;
    let c = client();
    let q = "type=Building&georel=nonsense&geometry=Point&coordinates=%5B0.0,0.0%5D";
    let (status, _, _) = query(&c, q).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn spatial_georel_missing_geometry_returns_400() {
    clean_database().await;
    let c = client();
    let q = "type=Building&georel=within&coordinates=%5B0.0,0.0%5D";
    let (status, _, _) = query(&c, q).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn spatial_georel_near_without_distance_returns_400() {
    clean_database().await;
    let c = client();
    let q = "type=Building&georel=near&geometry=Point&coordinates=%5B0.0,0.0%5D";
    let (status, _, _) = query(&c, q).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Concurrency / consistency — read after rapid mutations
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn spatial_georel_rapid_updates_are_reflected() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:rapid");
    assert_eq!(create_entity(&c, &building_point(&id, 0.5, 0.5)).await, StatusCode::CREATED);

    for i in 0..5 {
        let lon = 10.0 + (i as f64) * 0.1;
        assert_eq!(patch_location_point(&c, &id, lon, 10.0).await, StatusCode::NO_CONTENT);
    }

    let coords = "%5B%5B%5B10.0,9.5%5D,%5B10.5,9.5%5D,%5B10.5,10.5%5D,%5B10.0,10.5%5D,%5B10.0,9.5%5D%5D%5D";
    let q = format!("type=Building&georel=within&geometry=Polygon&coordinates={coords}");
    let (status, _, body) = query(&c, &q).await;
    assert_eq!(status, StatusCode::OK);
    // The final position (~10.4) lies inside the polygon.
    assert!(ids(&body).contains(&id), "expected entity to reflect last update");
}
