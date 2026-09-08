//! Verification-matrix row "Arbitrary `GeoProperty`".
//!
//! A geo-query names the `GeoProperty` it applies to (clause 4.10). Only that
//! attribute may participate, any of its dataset instances matching is enough,
//! an Entity without it never matches, and the topological plan must reach the
//! composite geometry index on the projection.

use crate::seeded_corpus::{explain, seed};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// `datasetId` of the second `entrance` instance, deliberately far away from
/// the default instance.
const REMOTE_DATASET: &str = "urn:ngsi-ld:Dataset:remote";

/// Entity carrying two named `GeoProperties`, one of them with two instances.
fn two_geoproperty_building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "entrance": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}},
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [20.0, 60.0]}, "datasetId": REMOTE_DATASET}
        ],
        "dock": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Entity carrying only the second `GeoProperty`.
fn dock_only_building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "dock": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Identifiers selected by one geo-query restricted to the two fixtures.
async fn selected(client: &TestClient, id_filter: &str, filter: &str) -> Vec<String> {
    let query = format!("type=Building&id={id_filter}&local=true&{filter}");
    let response = client
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("geo query failed");
    assert_eq!(response.status, StatusCode::OK, "{filter}: {:?}", response.body);
    let body = response.json_value().expect("query body must be JSON");
    body.as_array()
        .map(|entities| {
            entities
                .iter()
                .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn arbitrary_geoproperty_queries_apply_to_the_named_attribute_alone() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let client = scope.client();
    let with_entrance = scope.id("Building:geo-two-properties");
    let dock_only = scope.id("Building:geo-dock-only");
    for entity in [two_geoproperty_building(&with_entrance), dock_only_building(&dock_only)] {
        let created = client.post_entity(&entity).send().await.expect("create entity failed");
        assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    }
    let ids = format!("{},{}", urlencoding::encode(&with_entrance), urlencoding::encode(&dock_only));
    let berlin_polygon = "%5B%5B%5B12.9,51.9%5D,%5B13.1,51.9%5D,%5B13.1,52.1%5D,%5B12.9,52.1%5D,%5B12.9,51.9%5D%5D%5D";

    // Both fixtures carry a point at 13/52, but under different attributes.
    assert_eq!(
        selected(
            client,
            &ids,
            &format!("georel=within&geometry=Polygon&coordinates={berlin_polygon}&geoproperty=entrance")
        )
        .await,
        vec![with_entrance.clone()],
        "only the named GeoProperty participates"
    );
    assert_eq!(
        selected(
            client,
            &ids,
            &format!("georel=within&geometry=Polygon&coordinates={berlin_polygon}&geoproperty=dock")
        )
        .await,
        vec![dock_only.clone()],
        "the other GeoProperty selects the other Entity"
    );

    // An Entity that has no instance of the selected GeoProperty cannot match
    // any relation over it, including a negative one.
    let disjoint = selected(client, &ids, "georel=disjoint&geometry=Point&coordinates=%5B0.0,0.0%5D&geoproperty=entrance").await;
    assert_eq!(disjoint, vec![with_entrance.clone()], "a missing GeoProperty never matches");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn arbitrary_geoproperty_matches_on_any_dataset_instance_and_orders_by_distance() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geo-instances");
    let near = scope.id("Building:geo-instances-near");
    let created = client.post_entity(&two_geoproperty_building(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let neighbour = json!({
        "id": near,
        "type": "Building",
        "entrance": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.001, 52.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&neighbour).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let ids = format!("{},{}", urlencoding::encode(&id), urlencoding::encode(&near));

    // The default instance is at 13/52 and the named dataset at 20/60: either
    // one matching is enough.
    assert!(
        selected(
            client,
            &ids,
            "georel=near;maxDistance==1000&geometry=Point&coordinates=%5B20.0,60.0%5D&geoproperty=entrance"
        )
        .await
        .contains(&id),
        "any instance of the selected GeoProperty may satisfy the relation"
    );
    assert!(
        selected(
            client,
            &ids,
            "georel=near;minDistance==100000&geometry=Point&coordinates=%5B13.0,52.0%5D&geoproperty=entrance"
        )
        .await
        .contains(&id),
        "the remote instance satisfies a minimum-distance relation"
    );

    let ordered = selected(client, &ids, "orderBy=entrance;dist-asc&orderFrom=%5B13.001,52.0%5D").await;
    assert_eq!(
        ordered.first(),
        Some(&near),
        "distance ordering over a named GeoProperty must put the closest Entity first: {ordered:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn arbitrary_geoproperty_topological_queries_use_the_projection_geometry_index() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;

    let plan = explain(
        pool,
        &format!(
            "SELECT c.entity_key FROM current_attribute_values AS c \
             WHERE c.attribute_name_id = {} \
               AND ST_Intersects(c.value_geo, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326))",
            corpus.entrance()
        ),
    )
    .await;
    assert!(
        plan.contains("idx_cav_attr_geo"),
        "a topological query over a named GeoProperty must use the composite geometry index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}
