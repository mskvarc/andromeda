//! Staged construction of a temporal `EntityMap` (clause 5.14.5): the candidate
//! set S1, the value-filter result S2, the geo-filter result S3, the
//! scope-filter result S4, and the map built from S4. Each row observes the
//! stage boundary through map membership. All `T-LOCAL`.

use crate::{
    map::{
        entity_map_request::create_temporal_entity_map,
        temporal_vehicle::{BEFORE_2022, temporal_vehicle},
    },
    oracle::{assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Value, json};

/// A near geo query centred on the origin.
const NEAR_ORIGIN: &str = "georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B0%2C0%5D";

/// Create a temporal entity on the front broker.
async fn seed(client: &TestClient, body: &Value) {
    assert_status(&client.create_temporal_entity(body).await.expect("create temporal entity"), 201);
}

/// A temporal `Vehicle` located far from the origin.
fn far_vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [ { "type": "Property", "value": 20, "observedAt": "2021-01-01T00:00:00Z" } ],
        "location": { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [80, 80] }, "observedAt": "2021-01-01T00:00:00Z" },
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// A temporal `Vehicle` carrying a scope stamped inside the `BEFORE_2022`
/// window, so the temporal representation exposes the scope within the queried
/// interval (clause 5.14.5.4 evaluates the scope query against the selected
/// temporal evolution).
fn scoped_vehicle(id: &str, scope: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "scope": { "type": "Property", "value": scope, "observedAt": "2020-06-01T00:00:00Z" },
        "speed": [ { "type": "Property", "value": 90, "observedAt": "2021-01-01T00:00:00Z" } ],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// Whether the map keys the given entity id.
fn maps(created: &Value, entity_id: &str) -> bool {
    created["entityMap"].get(entity_id).is_some()
}

/// The full non-split local pipeline keeps only entities passing every stage
/// (clause 5.14.5). Confirms REQ-MAP-142.
#[tokio::test]
async fn emt_non_split_local_fully_filtered_s1_s4() {
    let topology = topology::single().await.expect("single-broker topology");
    let keep = topology.id("Vehicle:pipeline-keep");
    let drop = topology.id("Vehicle:pipeline-drop");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&keep, 90),
    )
    .await;
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&drop, 10),
    )
    .await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&q=speed%3E50&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(maps(&created, &keep), "the fully-passing entity is mapped");
    assert!(!maps(&created, &drop), "an entity failing a stage is dropped");
    topology.finish().await.expect("topology cleanup");
}

/// A values filter narrows S1 into S2 (clause 5.14.5). Confirms REQ-MAP-143.
#[tokio::test]
async fn emt_values_filter_produces_s2() {
    let topology = topology::single().await.expect("single-broker topology");
    let fast = topology.id("Vehicle:s2-fast");
    let slow = topology.id("Vehicle:s2-slow");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&fast, 90),
    )
    .await;
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&slow, 10),
    )
    .await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&q=speed%3E50&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(maps(&created, &fast) && !maps(&created, &slow), "the values filter yields S2");
    topology.finish().await.expect("topology cleanup");
}

/// Without a values filter S2 equals S1 (clause 5.14.5). Confirms REQ-MAP-144.
#[tokio::test]
async fn emt_no_values_filter_s2_equals_s1() {
    let topology = topology::single().await.expect("single-broker topology");
    let first = topology.id("Vehicle:s2-eq-1");
    let second = topology.id("Vehicle:s2-eq-2");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&first, 90),
    )
    .await;
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&second, 10),
    )
    .await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(
        maps(&created, &first) && maps(&created, &second),
        "without a values filter S2 retains all of S1"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A geo query narrows S2 into S3 (clause 5.14.5). Confirms REQ-MAP-145.
#[tokio::test]
async fn emt_geoquery_produces_s3() {
    let topology = topology::single().await.expect("single-broker topology");
    let near = topology.id("Vehicle:s3-near");
    let far = topology.id("Vehicle:s3-far");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&near, 20),
    )
    .await;
    seed(topology.broker(BrokerRole::A).expect("topology broker A").client(), &far_vehicle(&far)).await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&{NEAR_ORIGIN}&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(maps(&created, &near) && !maps(&created, &far), "the geo query yields S3");
    topology.finish().await.expect("topology cleanup");
}

/// Without a geo query S3 equals S2 (clause 5.14.5). Confirms REQ-MAP-146.
#[tokio::test]
async fn emt_no_geoquery_s3_equals_s2() {
    let topology = topology::single().await.expect("single-broker topology");
    let near = topology.id("Vehicle:s3-eq-near");
    let far = topology.id("Vehicle:s3-eq-far");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&near, 20),
    )
    .await;
    seed(topology.broker(BrokerRole::A).expect("topology broker A").client(), &far_vehicle(&far)).await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(maps(&created, &near) && maps(&created, &far), "without a geo query S3 retains all of S2");
    topology.finish().await.expect("topology cleanup");
}

/// A scope query narrows S3 into S4 (clause 5.14.5). Confirms REQ-MAP-147.
#[tokio::test]
async fn emt_scope_query_produces_s4() {
    let topology = topology::single().await.expect("single-broker topology");
    let inside = topology.id("Vehicle:s4-inside");
    let outside = topology.id("Vehicle:s4-outside");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &scoped_vehicle(&inside, "/Madrid/Center"),
    )
    .await;
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &scoped_vehicle(&outside, "/Barcelona"),
    )
    .await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&scopeQ=%2FMadrid%2FCenter&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(maps(&created, &inside) && !maps(&created, &outside), "the scope query yields S4");
    topology.finish().await.expect("topology cleanup");
}

/// Without a scope query S4 equals S3 (clause 5.14.5). Confirms REQ-MAP-148.
#[tokio::test]
async fn emt_no_scope_query_s4_equals_s3() {
    let topology = topology::single().await.expect("single-broker topology");
    let first = topology.id("Vehicle:s4-eq-1");
    let second = topology.id("Vehicle:s4-eq-2");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &scoped_vehicle(&first, "/Madrid/Center"),
    )
    .await;
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &scoped_vehicle(&second, "/Barcelona"),
    )
    .await;
    let created = json_body(
        &create_temporal_entity_map(
            topology.broker(BrokerRole::A).expect("topology broker A").client(),
            &format!("type=Vehicle&{BEFORE_2022}"),
        )
        .await,
    );
    assert!(maps(&created, &first) && maps(&created, &second), "without a scope query S4 retains all of S3");
    topology.finish().await.expect("topology cleanup");
}

/// The local map is created from S4 (clause 5.14.5). Confirms REQ-MAP-149.
#[tokio::test]
async fn emt_local_map_created_from_s4() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:s4-map");
    seed(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &temporal_vehicle(&entity_id, 90),
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type=Vehicle&q=speed%3E50&{BEFORE_2022}"),
    )
    .await;
    assert_status(&created, 201);
    assert!(maps(&json_body(&created), &entity_id), "the S4 survivors form the local map");
    topology.finish().await.expect("topology cleanup");
}
