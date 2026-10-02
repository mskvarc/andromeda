//! A geometry keeps the `bbox` and the foreign members it was written with.
//!
//! IETF RFC 7946 gives every geometry a `bbox` ("A `GeoJSON` object MAY have a
//! member named "bbox"", section 5) and lets it carry foreign members
//! ("Members not described by this specification ("foreign members") MAY be
//! used in a `GeoJSON` document", section 6.1). ETSI GS CIM 009 v1.9.1 clause
//! 4.7.2 accepts a geometry "meeting the syntax and restrictions mandated by
//! IETF RFC 7946", and clause 4.6.4 obliges the broker to "preserve the
//! representation of the content of the values provided [...] and return the
//! original content". The broker kept a geometry's type and positions only, so
//! both members were dropped on every write, Core and Temporal alike, and a
//! stored document holding them could not be read at all.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 answers an inadmissible value with.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// A square parcel with a `bbox` and two foreign members, one of them nested.
fn parcel_geometry() -> Value {
    json!({
        "type": "Polygon",
        "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]],
        "bbox": [0.0, 0.0, 4.0, 4.0],
        "plotName": "plot 7",
        "survey": {"year": 2024, "marks": [1, 2]}
    })
}

/// An elevated marker with a three-dimensional `bbox`.
fn marker_geometry() -> Value {
    json!({"type": "Point", "coordinates": [1.0, 1.0, 3.0], "bbox": [1.0, 1.0, 3.0, 1.0, 1.0, 3.0]})
}

/// A `Parcel` whose `area` is a `GeoProperty` and whose `marker` is a Property
/// holding a geometry.
fn parcel(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Parcel",
        "area": {"type": "GeoProperty", "value": parcel_geometry()},
        "marker": {"type": "Property", "value": marker_geometry()},
        "@context": [CORE_CONTEXT]
    })
}

/// `value` with every number read as the double it names, so two renderings
/// compare by value: the Temporal API spells an integral ordinate as an
/// integer, the Core API as a double.
fn by_value(value: &Value) -> Value {
    match value {
        Value::Number(number) => number.as_f64().map_or(Value::Null, Value::from),
        Value::Array(items) => Value::Array(items.iter().map(by_value).collect()),
        Value::Object(members) => Value::Object(members.iter().map(|(name, member)| (name.clone(), by_value(member))).collect()),
        Value::Null | Value::Bool(_) | Value::String(_) => value.clone(),
    }
}

/// Read `id` back as `accept`, under the core context.
async fn read(client: &TestClient, id: &str, query: &str, accept: &str) -> Value {
    let response = client.get_entity(id, query, accept, Some(CORE_CONTEXT)).await.expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve: {:?}", response.body);
    response.json_value().expect("a JSON entity")
}

/// Clause 4.6.4: the `bbox` and the foreign members come back on a normalized
/// read, on a simplified read and in the `GeoJSON` Feature of clause 4.5.16.2,
/// for a `GeoProperty` and for a Property whose value is a geometry.
#[tokio::test]
async fn regression_geojson_members_round_trip_through_every_core_representation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Parcel:members");

    let created = client.post_entity(&parcel(&id)).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let normalized = read(client, &id, "", "application/ld+json").await;
    assert_eq!(normalized["area"]["value"], parcel_geometry(), "{normalized}");
    assert_eq!(normalized["marker"]["value"], marker_geometry(), "{normalized}");

    let simplified = read(client, &id, "options=keyValues", "application/json").await;
    assert_eq!(simplified["area"], parcel_geometry(), "{simplified}");

    let feature = read(client, &id, "geometryProperty=area", "application/geo+json").await;
    assert_eq!(feature["geometry"], parcel_geometry(), "{feature}");

    scope.finish().await.expect("cleanup");
}

/// IETF RFC 7946 section 5: "The value of the bbox member MUST be an array of
/// length 2*n"; clauses 4.7.2 and 5.5.4 refuse a geometry that is not one with
/// `BadRequestData`.
#[tokio::test]
async fn regression_geojson_members_a_bbox_of_three_numbers_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Parcel:short-bbox");

    let entity = json!({
        "id": id,
        "type": "Parcel",
        "area": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0], "bbox": [0.0, 0.0, 4.0]}},
        "@context": [CORE_CONTEXT]
    });
    let refused = client.post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);
    let problem = refused.json_value().expect("a problem detail");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");
    assert!(
        problem["detail"].as_str().is_some_and(|detail| detail.contains("RFC 7946 section 5")),
        "{problem}"
    );

    scope.finish().await.expect("cleanup");
}

/// Clauses 5.6.11 and 4.6.4: an observation of a geometry with members keeps
/// them in the Temporal Evolution, so the history renders the geometry that was
/// observed.
#[tokio::test]
async fn regression_geojson_members_survive_a_temporal_write() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Parcel:temporal-members");

    let evolution = json!({
        "id": id,
        "type": "Parcel",
        "area": [{"type": "GeoProperty", "value": parcel_geometry(), "observedAt": "2026-01-01T00:00:00Z"}],
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_temporal_entity(&evolution).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);

    let history = client
        .get_temporal_entity(&id, "", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(history.status, StatusCode::OK, "temporal retrieve: {:?}", history.body);
    let history = history.json_value().expect("a temporal entity");
    let instances = history["area"].as_array().expect("the area history");
    assert_eq!(instances.len(), 1, "{history}");
    assert_eq!(by_value(&instances[0]["value"]), by_value(&parcel_geometry()), "{history}");

    // The current state the temporal write brought into being carries them
    // too (clause 5.6.11.4).
    let current = read(client, &id, "", "application/ld+json").await;
    assert_eq!(by_value(&current["area"]["value"]), by_value(&parcel_geometry()), "{current}");

    scope.finish().await.expect("cleanup");
}
