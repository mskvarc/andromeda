//! Entity fixtures and request helpers shared by the 3D coordinate suite.
//!
//! The canonical sample positions are a single lon/lat pillar observed at three
//! arities: `p_2d` (two ordinates), `p0` (altitude 0) and `p500` (altitude 500).
//! Every spatial predicate must treat all three as the same footprint
//! (NGSI-LD 4.10, OGC 06-103r4 6.1.15).

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};

/// Longitude shared by every pillar fixture.
pub const PILLAR_LONGITUDE: f64 = 13.4;

/// Latitude shared by every pillar fixture.
pub const PILLAR_LATITUDE: f64 = 52.52;

/// The pillar position with two ordinates only.
pub fn p_2d() -> Value {
    json!([PILLAR_LONGITUDE, PILLAR_LATITUDE])
}

/// The pillar position at ground level, expressed with three ordinates.
pub fn p0() -> Value {
    json!([PILLAR_LONGITUDE, PILLAR_LATITUDE, 0.0])
}

/// The pillar position 500 m above ground, expressed with three ordinates.
pub fn p500() -> Value {
    json!([PILLAR_LONGITUDE, PILLAR_LATITUDE, 500.0])
}

/// A `Building` whose `location` is a three-dimensional Point.
pub fn building_point_3d(id: &str, lon: f64, lat: f64, alt: f64) -> Value {
    building_with_location(id, &json!({"type": "Point", "coordinates": [lon, lat, alt]}))
}

/// A `Building` whose `location` is a two-dimensional Point.
pub fn building_point_2d(id: &str, lon: f64, lat: f64) -> Value {
    building_with_location(id, &json!({"type": "Point", "coordinates": [lon, lat]}))
}

/// A `Building` whose `location` is a Polygon built from the supplied rings.
pub fn building_polygon_3d(id: &str, coordinates: &Value) -> Value {
    building_with_location(id, &json!({"type": "Polygon", "coordinates": coordinates}))
}

/// A `Building` carrying an arbitrary `GeoJSON` geometry as its `location`.
pub fn building_with_location(id: &str, geometry: &Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": geometry},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A square ring at the given altitude, spanning one degree in each direction
/// around the pillar longitude/latitude.
pub fn pillar_ring_3d(altitude: f64) -> Value {
    json!([[
        [13.0, 52.0, altitude],
        [14.0, 52.0, altitude],
        [14.0, 53.0, altitude],
        [13.0, 53.0, altitude],
        [13.0, 52.0, altitude]
    ]])
}

/// The same square ring with two-ordinate positions.
pub fn pillar_ring_2d() -> Value {
    json!([[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]])
}

/// Creates an entity and asserts the broker accepted it.
pub async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "entity create must succeed");
}

/// Issues a GET entity query with the test-suite context applied.
pub async fn query(client: &TestClient, query_str: &str) -> (StatusCode, HeaderMap, Value) {
    let path = format!("/ngsi-ld/v1/entities?{query_str}");
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test context Link must be valid"))
        .send()
        .await
        .expect("query failed");
    let body = response.json_value().expect("response body must be valid JSON");
    (response.status, response.headers, body)
}

/// Retrieves an entity and returns its parsed body.
pub async fn get_entity(client: &TestClient, id: &str) -> (StatusCode, Value) {
    let response = client
        .get_entity(id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    let body = response.json_value().expect("retrieved entity must be valid JSON");
    (response.status, body)
}

/// Extracts the `location` `GeoProperty` coordinates from a normalised entity.
pub fn location_coordinates(entity: &Value) -> &Value {
    entity
        .pointer("/location/value/coordinates")
        .expect("entity must carry location.value.coordinates")
}

/// Extracts the coordinates of an arbitrary `GeoProperty` from a normalised entity.
pub fn geoproperty_coordinates<'a>(entity: &'a Value, attribute: &str) -> &'a Value {
    entity
        .pointer(&format!("/{attribute}/value/coordinates"))
        .expect("entity must carry the requested GeoProperty coordinates")
}

/// Collects the entity identifiers present in a query result array.
pub fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

/// Builds a comma-separated, percent-encoded `id` query parameter value.
pub fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// Percent-encodes a JSON value for use as a query parameter.
pub fn encode_json(value: &Value) -> String {
    urlencoding::encode(&value.to_string()).into_owned()
}

/// Compares two `GeoJSON` coordinate trees by numeric value.
///
/// A JSON round-trip may render `13.0` as `13`, which `serde_json` treats as a
/// different `Number` even though RFC 7946 gives the two identical meaning.
/// Coordinate assertions therefore compare numerically rather than structurally.
pub fn coordinates_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left_items), Value::Array(right_items)) => {
            left_items.len() == right_items.len() && left_items.iter().zip(right_items).all(|(a, b)| coordinates_equal(a, b))
        }
        (Value::Number(left_number), Value::Number(right_number)) => match (left_number.as_f64(), right_number.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
            (Some(_) | None, _) => false,
        },
        // Any other pairing is compared structurally: coordinate trees contain
        // only arrays and numbers, so this is the "not a coordinate tree" case.
        (Value::Array(_) | Value::Number(_) | Value::Null | Value::Bool(_) | Value::String(_) | Value::Object(_), other) => left == other,
    }
}

/// Asserts two coordinate trees are numerically equal, reporting both on failure.
pub fn assert_coordinates_eq(actual: &Value, expected: &Value) {
    assert!(coordinates_equal(actual, expected), "coordinates differ: got {actual}, expected {expected}");
}
