//! `near` measures the minimum distance between the geometries, on the WGS 84
//! spheroid.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.10 matches `near;maxDistance` against "the
//! buffer geometric object (as defined by [14])" of the reference geometry and
//! `near;minDistance` against its complement, and OGC 06-103r4 clause 6.1.2.4
//! defines both `Buffer` and `Distance` through "the shortest distance between
//! any two Points in the two geometric objects". A building whose nearest wall
//! is 100 m from the reference point is therefore within 150 m, wherever its
//! centroid lies, and a point inside a polygon is zero metres from it. The
//! distance is measured on the WGS 84 spheroid, the datum of every `GeoJSON`
//! position (RFC 7946 section 4) and the one `geography` measures on.
//!
//! The in-memory spatial index once refined `near` by the distance between
//! centroids on a sphere before the store re-checked it, and subscriptions were
//! matched by that reading alone, so a query answered the intersection of the
//! two readings and a subscription answered the wrong one. Every query here
//! names its entities by `id`, so only the test's own entities count.
//!
//! The replica cases run against the base tier's second broker, which serves
//! the same database.
//!
//! Naming convention: `regression_near_minimum_distance_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    large_entity_budget::{CORE_CONTEXT, send_json},
    large_entity_replicas::second_replica,
    notification_adapter::NotificationAdapter,
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::{environment::TestEnvironment, tenant_scoped_environment::TenantScopedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The reference point every Bremen fixture is placed around.
const BREMEN: (f64, f64) = (8.8072, 53.0758);

/// How long an owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(30);

/// How long the absence of a notification is watched for.
const QUIET: Duration = Duration::from_secs(8);

/// The `GeoJSON` Polygon whose rings are the given longitude/latitude boxes.
fn boxes_polygon(rings: &[(f64, f64, f64, f64)]) -> Value {
    json!({ "type": "Polygon", "coordinates": rings.iter().map(|&box_ring| ring(box_ring)).collect::<Vec<_>>() })
}

/// The closed ring around one longitude/latitude box.
fn ring((west, south, east, north): (f64, f64, f64, f64)) -> Value {
    json!([[west, south], [east, south], [east, north], [west, north], [west, south]])
}

/// A square whose west wall is 100.06 m east of Bremen and whose centroid is
/// 170.10 m east of it.
fn east_square() -> Value {
    boxes_polygon(&[(8.808_693, 53.075_171, 8.810_783, 53.076_429)])
}

/// The Point `GeoJSON` coordinates of Bremen.
fn bremen() -> Value {
    json!([BREMEN.0, BREMEN.1])
}

/// Create `id` of `entity_type` through `client`, its `location` being
/// `geometry`.
async fn create_located(client: &TestClient, id: &str, entity_type: &str, geometry: &Value) {
    let entity = json!({"id": id, "type": entity_type, "location": {"type": "GeoProperty", "value": geometry}});
    let created = send_json(client, Method::POST, "/ngsi-ld/v1/entities", entity.to_string()).await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// The ids among `ids` of `entity_type` that `georel` against the reference
/// geometry selects, sorted.
async fn near_ids(client: &TestClient, entity_type: &str, ids: &[&str], georel: &str, geometry: &str, coordinates: &Value) -> Vec<String> {
    let path = format!(
        "/ngsi-ld/v1/entities?type={entity_type}&id={}&georel={}&geometry={geometry}&coordinates={}",
        ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(","),
        urlencoding::encode(georel),
        urlencoding::encode(&coordinates.to_string()),
    );
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/json")
        .send()
        .await
        .expect("query entities request");
    let body = response.json_value().expect("a JSON answer");
    assert_eq!(response.status, StatusCode::OK, "{georel}: {body}");
    let mut found: Vec<String> = body
        .as_array()
        .expect("an array of entities")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    found.sort();
    found
}

/// Subscribe `client` to `near` matches of `id` of `entity_type` around
/// Bremen, notifying `adapter`.
async fn subscribe_near(client: &TestClient, adapter: &NotificationAdapter, subscription: &str, entity_type: &str, id: &str, georel: &str) {
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription,
            "type": "Subscription",
            "entities": [{"type": entity_type, "id": id}],
            "geoQ": {"georel": georel, "geometry": "Point", "coordinates": bremen()},
            "notification": {"endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
}

#[tokio::test]
async fn regression_near_minimum_distance_matches_a_polygon_by_its_nearest_wall() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearBuilding");
    let id = scope.id("NearBuilding:east-square");
    create_located(client, &id, &entity_type, &east_square()).await;

    let ids = [id.as_str()];
    let reference = bremen();
    let near = |georel: &'static str| near_ids(client, &entity_type, &ids, georel, "Point", &reference);
    assert_eq!(near("near;maxDistance==150").await, vec![id.clone()], "the nearest wall is 100.06 m away");
    assert_eq!(near("near;maxDistance==90").await, Vec::<String>::new());
    assert_eq!(near("near;minDistance==150").await, Vec::<String>::new());
    assert_eq!(near("near;minDistance==90").await, vec![id.clone()]);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_puts_a_point_inside_a_polygon_at_zero() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearBuilding");
    let id = scope.id("NearBuilding:around-bremen");
    // Its centroid is 995.24 m from Bremen.
    create_located(client, &id, &entity_type, &boxes_polygon(&[(8.8069, 53.0753, 8.8372, 53.0763)])).await;

    let ids = [id.as_str()];
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;maxDistance==500", "Point", &bremen()).await,
        vec![id.clone()]
    );
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;minDistance==500", "Point", &bremen()).await,
        Vec::<String>::new()
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_measures_a_hole_to_its_boundary() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearBuilding");
    let id = scope.id("NearBuilding:holed");
    let (x, y) = BREMEN;
    // Bremen lies in the hole, 100.06 m from its nearest wall; the polygon's
    // centroid is Bremen itself.
    let holed = boxes_polygon(&[
        (x - 0.015, y - 0.009, x + 0.015, y + 0.009),
        (x - 0.001_493, y - 0.000_898, x + 0.001_493, y + 0.000_898),
    ]);
    create_located(client, &id, &entity_type, &holed).await;

    let ids = [id.as_str()];
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;maxDistance==50", "Point", &bremen()).await,
        Vec::<String>::new()
    );
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;maxDistance==150", "Point", &bremen()).await,
        vec![id.clone()]
    );
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;minDistance==50", "Point", &bremen()).await,
        vec![id.clone()]
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_matches_a_line_through_its_nearest_edge() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearRoad");
    let id = scope.id("NearRoad:east-line");
    // Its nearest vertex is 316.70 m away, the foot on its first edge 100.06 m.
    let line = json!({"type": "LineString", "coordinates": [[8.808_693, 53.0731], [8.808_693, 53.0785], [8.817_693, 53.0785]]});
    create_located(client, &id, &entity_type, &line).await;

    assert_eq!(
        near_ids(client, &entity_type, &[id.as_str()], "near;maxDistance==150", "Point", &bremen()).await,
        vec![id.clone()]
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_matches_a_multi_polygon_by_its_nearest_member() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearBuilding");
    let id = scope.id("NearBuilding:pair");
    // The second member's nearest wall is 2 003.89 m away, and the planar
    // centroid of the pair lies about a kilometre west of Bremen.
    let pair = json!({
        "type": "MultiPolygon",
        "coordinates": [[ring((8.808_693, 53.075_171, 8.810_783, 53.076_429))], [ring((8.7753, 53.0748, 8.7773, 53.0768))]]
    });
    create_located(client, &id, &entity_type, &pair).await;

    assert_eq!(
        near_ids(client, &entity_type, &[id.as_str()], "near;maxDistance==150", "Point", &bremen()).await,
        vec![id.clone()]
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_measures_across_the_antimeridian() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearBuilding");
    let id = scope.id("NearBuilding:taveuni");
    // Cut at the antimeridian as RFC 7946 section 3.1.9 asks: 212.97 m from the
    // reference across it, 692.16 m by its planar centroid.
    let cut = json!({"type": "MultiPolygon", "coordinates": [[ring((-179.999, -17.0005, -179.990, -16.9995))]]});
    create_located(client, &id, &entity_type, &cut).await;

    assert_eq!(
        near_ids(client, &entity_type, &[id.as_str()], "near;maxDistance==300", "Point", &json!([179.999, -17.0])).await,
        vec![id.clone()]
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_measures_from_a_polygon_reference() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearStop");
    let id = scope.id("NearStop:east");
    // 90.07 m from the reference square's east edge, 140.07 m from its centre.
    create_located(client, &id, &entity_type, &json!({"type": "Point", "coordinates": [8.809_290, BREMEN.1]})).await;

    let square = json!([ring((8.806_454, 53.075_351, 8.807_946, 53.076_249))]);
    assert_eq!(
        near_ids(client, &entity_type, &[id.as_str()], "near;maxDistance==100", "Polygon", &square).await,
        vec![id.clone()]
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_measures_on_the_spheroid() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("NearStop");
    let id = scope.id("NearStop:north-of-origin");
    // 5 000.17 m north of the origin on the spheroid; the sphere says 5 028.24 m.
    create_located(client, &id, &entity_type, &json!({"type": "Point", "coordinates": [0.0, 0.045_22]})).await;

    let ids = [id.as_str()];
    let origin = json!([0.0, 0.0]);
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;maxDistance==5014", "Point", &origin).await,
        vec![id.clone()]
    );
    assert_eq!(
        near_ids(client, &entity_type, &ids, "near;minDistance==5014", "Point", &origin).await,
        Vec::<String>::new()
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_near_minimum_distance_notifies_by_the_nearest_wall() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let client = scope.client();
    let entity_type = scope.entity_type("NearBuilding");
    let id = scope.id("NearBuilding:notified");
    let within = scope.id("Subscription:near-max-150");
    let beyond = scope.id("Subscription:near-min-150");
    subscribe_near(client, &adapter, &within, &entity_type, &id, "near;maxDistance==150").await;
    subscribe_near(client, &adapter, &beyond, &entity_type, &id, "near;minDistance==150").await;

    create_located(client, &id, &entity_type, &east_square()).await;

    let notification = adapter.wait_for_subscription_notification(&within, ARRIVAL).await;
    assert_eq!(notification.body["data"][0]["id"], id.as_str(), "{}", notification.body);
    adapter.wait_for_no_subscription_notification(&beyond, QUIET).await;
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Both replicas answer `near` alike, and a write one replica committed is
/// found through the other.
#[tokio::test]
async fn regression_near_minimum_distance_is_answered_alike_by_both_replicas() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let first = scope.client();
    let second = second_replica(&scope);
    let entity_type = "Building";
    // The tenant comes into being through the first replica, and the second
    // builds its runtime and seeds its spatial index before the probe write.
    let seed = scope.id("Building:seed");
    create_located(first, &seed, entity_type, &json!({"type": "Point", "coordinates": [9.5, BREMEN.1]})).await;
    let read = second.get_entity(&seed, "", "application/json", None).await.expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
    assert_eq!(
        near_ids(&second, entity_type, &[seed.as_str()], "near;maxDistance==1000", "Point", &bremen()).await,
        Vec::<String>::new()
    );

    let id = scope.id("Building:east-square");
    create_located(first, &id, entity_type, &east_square()).await;

    let ids = [id.as_str()];
    for (replica, name) in [(&second, "second"), (first, "first")] {
        for (georel, expected) in [
            ("near;maxDistance==150", vec![id.clone()]),
            ("near;maxDistance==175", vec![id.clone()]),
            ("near;minDistance==150", Vec::new()),
        ] {
            assert_eq!(
                near_ids(replica, entity_type, &ids, georel, "Point", &bremen()).await,
                expected,
                "{georel} through the {name} replica"
            );
        }
    }
    scope.finish().await.expect("cleanup");
}

/// A replica matches a write against subscriptions the other replica created.
/// The first replica builds its runtime only after both subscriptions exist, so
/// it loads them rather than depending on a change it never saw.
#[tokio::test]
async fn regression_near_minimum_distance_notifies_alike_from_either_replica() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let adapter = NotificationAdapter::start().await;
    let first = scope.client();
    let second = second_replica(&scope);
    let entity_type = "Building";
    let id = scope.id("Building:east-square");
    create_located(
        &second,
        &scope.id("Building:seed"),
        entity_type,
        &json!({"type": "Point", "coordinates": [9.5, BREMEN.1]}),
    )
    .await;
    let within = scope.id("Subscription:replica-near-max-150");
    let beyond = scope.id("Subscription:replica-near-min-150");
    subscribe_near(&second, &adapter, &within, entity_type, &id, "near;maxDistance==150").await;
    subscribe_near(&second, &adapter, &beyond, entity_type, &id, "near;minDistance==150").await;

    create_located(first, &id, entity_type, &east_square()).await;

    let notification = adapter.wait_for_subscription_notification(&within, ARRIVAL).await;
    assert_eq!(notification.body["data"][0]["id"], id.as_str(), "{}", notification.body);
    adapter.wait_for_no_subscription_notification(&beyond, QUIET).await;
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup");
}
