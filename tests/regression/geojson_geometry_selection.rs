//! The clause 4.5.16.1 top-level `geometry` field selection, end to end.
//!
//! With several instances a `datasetId` selects one, otherwise the default instance is selected. An entity lacking the `GeoProperty` as specified, or holding no valid `GeoJSON` geometry, answers `geometry: null`. Clause 4.5.17.1 sends the simplified rendering back to the same algorithm, so every representation must pick the same instance.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The `GeoJSON` media type clause 4.5.16 answers a Feature in.
const GEO_JSON: &str = "application/geo+json";

/// The `datasetId` of the first dataset-scoped instance of every fixture.
const DATASET_ONE: &str = "urn:ngsi-ld:Dataset:one";

/// The `datasetId` of the second dataset-scoped instance of every fixture.
const DATASET_TWO: &str = "urn:ngsi-ld:Dataset:two";

/// A `GeoJSON` Point at `[x, 0.0]`.
fn point(x: f64) -> Value {
    json!({"type": "Point", "coordinates": [x, 0.0]})
}

/// One `GeoProperty` instance carrying `geometry`, optionally dataset-scoped.
fn instance(geometry: &Value, dataset_id: Option<&str>) -> Value {
    match dataset_id {
        Some(dataset_id) => json!({"type": "GeoProperty", "value": geometry, "datasetId": dataset_id}),
        None => json!({"type": "GeoProperty", "value": geometry}),
    }
}

/// A `Building` whose `location` carries `instances`.
fn building_with_location(id: &str, instances: &[Value]) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": instances,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create one entity and assert it was stored.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// The `GeoJSON` Feature the broker answers for `id` under `query`.
async fn feature(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, GEO_JSON, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve '{query}': {:?}", response.body);
    let body = response.json_value().expect("the Feature is JSON");
    assert_eq!(body["type"], json!("Feature"), "clause 4.5.16.2 answers a Feature: {body}");
    body
}

/// The simplified rendering is asked for twice. Clause 4.21 pruning routes it through the serialized reading of the entity, where the clause 4.5.4 `dataset` map is built for a multi-instance attribute, so the two spellings exercise the two readings of one rendering. Clause 4.5.17.1 gives both the same geometry.
const GEO_JSON_QUERIES: [(&str, &str); 3] = [
    ("normalized", ""),
    ("keyValues", "options=keyValues"),
    ("keyValues with a clause 4.21 projection", "options=keyValues&omit=scope"),
];

/// Clause 4.5.16.1: with multiple instances and no `datasetId`, the default instance is selected. Clause 4.5.17.1 binds the simplified rendering to the same algorithm, so the representation cannot change which instance the geometry comes from.
#[tokio::test]
async fn regression_geojson_geometry_the_default_instance_is_selected_in_every_representation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometry-default-instance");

    create_entity(
        client,
        &building_with_location(
            &id,
            &[
                instance(&point(1.0), Some(DATASET_ONE)),
                instance(&point(9.0), None),
                instance(&point(2.0), Some(DATASET_TWO)),
            ],
        ),
    )
    .await;

    for (label, query) in GEO_JSON_QUERIES {
        let body = feature(client, &id, query).await;
        assert_eq!(
            body["geometry"],
            point(9.0),
            "clause 4.5.16.1 selects the default instance under {label}: {body}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.16.1 admits two outcomes for a multi-instance `GeoProperty`, the instance a `datasetId` named or the default instance, and neither exists here. The entity lacks "the `GeoProperty` as specified", which the clause answers with a null geometry, never with an arbitrary instance whose identity would depend on row order.
#[tokio::test]
async fn regression_geojson_geometry_no_default_instance_and_no_dataset_id_renders_a_null_geometry() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometry-no-default-instance");

    create_entity(
        client,
        &building_with_location(&id, &[instance(&point(1.0), Some(DATASET_ONE)), instance(&point(2.0), Some(DATASET_TWO))]),
    )
    .await;

    for (label, query) in GEO_JSON_QUERIES {
        let body = feature(client, &id, query).await;
        assert_eq!(
            body["geometry"],
            Value::Null,
            "clause 4.5.16.1 leaves the geometry undefined under {label}: {body}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.16.1: a `datasetId` defines which instance is selected, in place of the default one.
#[tokio::test]
async fn regression_geojson_geometry_a_named_dataset_id_selects_that_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometry-named-dataset");

    create_entity(
        client,
        &building_with_location(
            &id,
            &[
                instance(&point(9.0), None),
                instance(&point(1.0), Some(DATASET_ONE)),
                instance(&point(2.0), Some(DATASET_TWO)),
            ],
        ),
    )
    .await;

    let named = feature(client, &id, &format!("datasetId={}", urlencoding::encode(DATASET_TWO))).await;
    assert_eq!(
        named["geometry"],
        point(2.0),
        "clause 4.5.16.1 selects the instance the datasetId named: {named}"
    );

    let default = feature(client, &id, "").await;
    assert_eq!(
        default["geometry"],
        point(9.0),
        "clause 4.5.16.1 selects the default instance when no datasetId was specified: {default}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.16.1: a selected member whose value is not a valid `GeoJSON` geometry yields a null geometry, because IETF RFC 7946 requires the `geometry` member to be a geometry or null.
#[tokio::test]
async fn regression_geojson_geometry_a_value_that_is_not_a_geometry_renders_a_null_geometry() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometry-not-a-geometry");

    let entity = json!({
        "id": &id,
        "type": "Building",
        "location": [instance(&point(9.0), None)],
        "buildingNote": {"type": "Property", "value": "not a geometry"},
        "buildingCount": {"type": "Property", "value": 42},
        "buildingShape": {"type": "Property", "value": {"type": "Point"}},
        "buildingCollection": {"type": "Property", "value": {"type": "GeometryCollection", "geometries": []}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create_entity(client, &entity).await;

    for named in ["buildingNote", "buildingCount", "buildingShape", "buildingCollection"] {
        for (label, query) in GEO_JSON_QUERIES {
            let request = if query.is_empty() {
                format!("geometryProperty={named}")
            } else {
                format!("{query}&geometryProperty={named}")
            };
            let body = feature(client, &id, &request).await;
            assert_eq!(
                body["geometry"],
                Value::Null,
                "clause 4.5.16.1 leaves the geometry undefined for {named} under {label}: {body}"
            );
        }
    }

    // Control: the same entity answers with a geometry when the selected attribute holds one.
    let located = feature(client, &id, "").await;
    assert_eq!(located["geometry"], point(9.0), "the control must still carry its geometry: {located}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.16.1: an instance carrying a different `datasetId` is not the instance that was specified, so the geometry is undefined. A lone instance is rendered as an object (clause 4.5.5) rather than an array, and must be subject to the same selector.
#[tokio::test]
async fn regression_geojson_geometry_a_lone_instance_is_not_selected_by_another_dataset_id() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometry-lone-dataset-instance");

    create_entity(client, &building_with_location(&id, &[instance(&point(1.0), Some(DATASET_ONE))])).await;

    let other = feature(client, &id, &format!("datasetId={}", urlencoding::encode(DATASET_TWO))).await;
    assert_eq!(
        other["geometry"],
        Value::Null,
        "clause 4.5.16.1: the entity lacks the GeoProperty as specified: {other}"
    );

    let named = feature(client, &id, &format!("datasetId={}", urlencoding::encode(DATASET_ONE))).await;
    assert_eq!(named["geometry"], point(1.0), "the instance the datasetId named is selected: {named}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.16.1: with a `geometryProperty` naming another `GeoProperty`, the instance selection runs over that attribute; without it "the default name of `location` shall be used".
#[tokio::test]
async fn regression_geojson_geometry_a_named_geometry_property_uses_its_own_default_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geometry-named-property");

    let entity = json!({
        "id": &id,
        "type": "Building",
        "location": [instance(&point(9.0), None), instance(&point(1.0), Some(DATASET_ONE))],
        "observationSpace": [instance(&point(5.0), Some(DATASET_ONE)), instance(&point(7.0), None)],
        "@context": [TEST_SUITE_CONTEXT]
    });
    create_entity(client, &entity).await;

    let observed = feature(client, &id, "geometryProperty=observationSpace").await;
    assert_eq!(
        observed["geometry"],
        point(7.0),
        "clause 4.5.16.1 selects the named GeoProperty's default instance: {observed}"
    );

    scope.finish().await.expect("cleanup failed");
}
