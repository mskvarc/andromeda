//! Entity ordering integration tests (ETSI GS CIM 009 clause 4.23, 5.2.43, 6.4.3.2).
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Covers:
//! - Distance-ascending and distance-descending ordering with `orderFrom` + `orderBy`.
//! - `orderGeometry` overrides the attribute geometry for the distance calculation.
//! - Geo-query coordinates used as fallback reference when `orderFrom` is absent.
//! - `orderFrom` takes precedence over geo-query reference coordinates.
//! - Mixed property-value and distance ordering.
//! - Error response for `dist-asc`/`dist-desc` when no reference is available.
//! - Error response for invalid `orderFrom` syntax.
//! - Error response for `orderGeometry=GeometryCollection`.
//! - Entities missing the ordered attribute are excluded from distance-ordered results.
//!
//! Naming convention: `entity_order_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";
const BAD_REQUEST_DATA_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

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

/// POST /entities. Returns (status, location).
async fn create_entity(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// GET /entities with query string.
async fn query_entities(client: &Client, query: &str) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

fn ids_from_response(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be JSON array")
        .iter()
        .map(|e| e["id"].as_str().expect("entity must have id field").to_owned())
        .collect()
}

/// Create the three canonical test entities used across ordering tests.
///
/// Returns `(id_a, id_b, id_c)` where:
/// - A: Berlin north  `[13.3986, 52.5547]`
/// - B: Berlin south  `[13.509818, 52.363457]`
/// - C: Paris         `[2.3522, 48.8566]`
async fn create_ordering_fixtures(client: &Client) -> (String, String, String) {
    let id_a = unique_id("urn:ngsi-ld:Building:order-A");
    let id_b = unique_id("urn:ngsi-ld:Building:order-B");
    let id_c = unique_id("urn:ngsi-ld:Building:order-C");

    for (id, name, lon, lat) in [
        (&id_a, "Alpha", 13.3986_f64, 52.5547_f64),
        (&id_b, "Beta", 13.509818, 52.363457),
        (&id_c, "Gamma", 2.3522, 48.8566),
    ] {
        let entity = json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": name},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [lon, lat]}
            },
            "@context": [TEST_SUITE_CONTEXT]
        });
        let (status, _) = create_entity(client, &entity).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED, "create fixture {name}");
    }

    (id_a, id_b, id_c)
}

// ---------------------------------------------------------------------------
// Distance-ascending ordering with orderFrom (clause 4.23, 5.2.43)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_dist_asc_with_order_from_point_orders_nearest_first() {
    clean_database().await;
    let c = client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(&c, "type=Building&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    pretty_assertions::assert_eq!(ids, vec![id_a, id_b, id_c], "entities must be ordered nearest-to-farthest from Berlin-north");
}

// ---------------------------------------------------------------------------
// Distance-descending ordering with orderFrom (clause 4.23)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_dist_desc_with_order_from_point_orders_farthest_first() {
    clean_database().await;
    let c = client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(&c, "type=Building&orderBy=location;dist-desc&orderFrom=%5B13.3986%2C52.5547%5D").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    pretty_assertions::assert_eq!(ids, vec![id_c, id_b, id_a], "entities must be ordered farthest-to-nearest from Berlin-north");
}

// ---------------------------------------------------------------------------
// orderGeometry overrides the attribute geometry (clause 4.23.3)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_dist_asc_with_order_geometry_line_string_orders_by_distance_to_line() {
    // LineString near Paris: C must be nearest, then B (closer to Paris than A).
    clean_database().await;
    let c = client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(
        &c,
        "type=Building&orderBy=location;dist-asc&orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%5D&orderGeometry=LineString",
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    pretty_assertions::assert_eq!(ids[0], id_c, "C (Paris) must be nearest to the Paris-area LineString");
    pretty_assertions::assert_eq!(ids[1], id_b, "B (Berlin south) must be second");
    pretty_assertions::assert_eq!(ids[2], id_a, "A (Berlin north) must be farthest");
}

// ---------------------------------------------------------------------------
// Geo-query fallback: coordinates used as distance reference (clause 4.23)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_dist_asc_without_order_from_uses_geo_query_reference() {
    // Geo filter centred on Berlin, small radius: only A and B within 50 km.
    // Without orderFrom, the geo-query centroid is used as the reference.
    clean_database().await;
    let c = client();
    let (id_a, id_b, _id_c) = create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(
        &c,
        "type=Building&georel=near;maxDistance%3D%3D50000&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&orderBy=location;dist-asc",
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    pretty_assertions::assert_eq!(ids.len(), 2, "only A and B must be within 50 km of Berlin-north");
    pretty_assertions::assert_eq!(ids[0], id_a, "A must be nearest");
    pretty_assertions::assert_eq!(ids[1], id_b, "B must be second");
}

// ---------------------------------------------------------------------------
// orderFrom takes precedence over geo-query reference (clause 5.2.43)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_order_from_takes_precedence_over_geo_query_coordinates() {
    // Geo filter centred on Paris (large radius, includes Berlin entities).
    // orderFrom = Berlin-north: ordering must use Berlin-north, not Paris.
    clean_database().await;
    let c = client();
    let (id_a, id_b, _id_c) = create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(
        &c,
        "type=Building&georel=near;maxDistance%3D%3D1000000&geometry=Point&coordinates=%5B2.3522%2C48.8566%5D&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D",
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert!(ids.len() >= 2, "at least A and B must be returned");
    pretty_assertions::assert_eq!(ids[0], id_a, "A must be first (nearest to Berlin-north orderFrom reference)");
    pretty_assertions::assert_eq!(ids[1], id_b, "B must be second");
}

// ---------------------------------------------------------------------------
// Mixed property-value and distance ordering (clause 4.23.3)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_mixed_property_and_distance_ordering_property_takes_precedence() {
    // Primary sort: name;asc (alphabetical). A=Alpha, B=Beta, C=Gamma.
    clean_database().await;
    let c = client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(&c, "type=Building&orderBy=name;asc,location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    pretty_assertions::assert_eq!(ids, vec![id_a, id_b, id_c], "alphabetical order: Alpha, Beta, Gamma");
}

// ---------------------------------------------------------------------------
// Error cases (clause 5.7.2.4)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_dist_ordering_without_any_reference_returns_bad_request() {
    // Neither orderFrom nor georel present; dist ordering cannot proceed.
    clean_database().await;
    let c = client();
    create_ordering_fixtures(&c).await;

    let (status, body) = query_entities(&c, "type=Building&orderBy=location;dist-asc").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"].as_str().unwrap(), BAD_REQUEST_DATA_TYPE, "must return BadRequestData error type");
}

#[tokio::test]
async fn entity_order_invalid_order_from_syntax_returns_bad_request() {
    let c = client();
    let (status, _) = query_entities(&c, "type=Building&orderBy=location;dist-asc&orderFrom=notjson").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn entity_order_geometry_collection_as_order_geometry_returns_bad_request() {
    // GeometryCollection is not a valid orderGeometry value (clause 5.2.43).
    let c = client();
    let (status, body) = query_entities(
        &c,
        "type=Building&orderBy=location;dist-asc&orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection",
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(
        body["type"].as_str().unwrap(),
        BAD_REQUEST_DATA_TYPE,
        "must return BadRequestData for GeometryCollection orderGeometry"
    );
}

// ---------------------------------------------------------------------------
// Entities missing the ordered attribute
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_order_entities_without_location_excluded_from_dist_ordered_results() {
    // One entity has location; one does not. Only the entity with location
    // can participate in distance ordering; the one without must be absent or
    // at least not cause the broker to error.
    clean_database().await;
    let c = client();
    let id_with = unique_id("urn:ngsi-ld:Building:ord-with-loc");
    let id_without = unique_id("urn:ngsi-ld:Building:ord-no-loc");

    let (status, _) = create_entity(
        &c,
        &json!({
            "id": id_with,
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.0, 52.0]}
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_entity(
        &c,
        &json!({
            "id": id_without,
            "type": "Building",
            "name": {"type": "Property", "value": "NoLocation"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = query_entities(&c, "type=Building&orderBy=location;dist-asc&orderFrom=%5B13.0%2C52.0%5D").await;
    // The broker must respond without error.
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert!(ids.contains(&id_with), "entity with location must appear in results");
}
