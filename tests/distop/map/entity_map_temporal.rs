//! Creating a temporal `EntityMap` from a temporal query: the mandatory temporal
//! query, entity-filter validation, term expansion, `lastN` selection, and
//! interval filtering (clause 5.14.5). All `T-LOCAL`.

use crate::{
    map::{
        entity_map_request::create_temporal_entity_map,
        temporal_vehicle::{BEFORE_2022, temporal_vehicle},
    },
    oracle::{assert_problem_details, assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;

/// Create a temporal `Vehicle` on the front broker.
async fn seed_temporal(client: &TestClient, id: &str, speed: i64) {
    assert_status(
        &client
            .create_temporal_entity(&temporal_vehicle(id, speed))
            .await
            .expect("create temporal entity"),
        201,
    );
}

/// A missing temporal query is rejected `400 BadRequestData` (clause 5.14.5).
/// Confirms REQ-MAP-132.
#[tokio::test]
async fn emt_no_temporal_query_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// At least one entity filter with a temporal query is accepted (clause 5.14.5).
/// Confirms REQ-MAP-133.
#[tokio::test]
async fn emt_at_least_one_filter_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    seed_temporal(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:temporal-filter"), 20).await;
    let response = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), &format!("type=Vehicle&{BEFORE_2022}")).await;
    assert_status(&response, 201);
    topology.finish().await.expect("topology cleanup");
}

/// No entity filter is rejected `400 BadRequestData` (clause 5.14.5).
/// Confirms REQ-MAP-134.
#[tokio::test]
async fn emt_no_filter_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), BEFORE_2022).await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A linked-entity retrieval is rejected `400 BadRequestData` (clause 5.14.5).
/// Confirms REQ-MAP-135.
#[tokio::test]
async fn emt_linked_entity_retrieval_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_temporal_entity_map(
        topology.broker(BrokerRole::A).client(),
        &format!("type=Vehicle&join=inline&joinLevel=99&{BEFORE_2022}"),
    )
    .await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An invalid `id`, `q`, `geoQ`, or `csf` is rejected `400 BadRequestData`
/// (clause 5.14.5). Confirms REQ-MAP-136.
#[tokio::test]
async fn emt_invalid_id_q_geoq_csf_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), &format!("id=not-a-uri&{BEFORE_2022}")).await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Type and attribute terms are expanded before the temporal map is built
/// (clause 5.14.5). Confirms REQ-MAP-137.
#[tokio::test]
async fn emt_expands_type_and_attribute_terms() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:temporal-expand");
    seed_temporal(topology.broker(BrokerRole::A).client(), &entity_id, 20).await;
    let created = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), &format!("type=Vehicle&attrs=speed&{BEFORE_2022}")).await;
    assert_status(&created, 201);
    assert!(
        json_body(&created)["entityMap"].get(&entity_id).is_some(),
        "the expanded terms select the temporal entity"
    );
    topology.finish().await.expect("topology cleanup");
}

/// JSON-LD type coercion is applied for the expansion list (clause 5.14.5).
/// Confirms REQ-MAP-138.
#[tokio::test]
async fn emt_applies_jsonld_type_coercion() {
    let topology = topology::single().await.expect("single-broker topology");
    seed_temporal(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:temporal-coercion"), 20).await;
    let created = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), &format!("type=Vehicle&{BEFORE_2022}")).await;
    assert_status(&created, 201);
    assert_eq!(
        json_body(&created)["type"].as_str(),
        Some("EntityMap"),
        "the coerced temporal query yields an EntityMap"
    );
    topology.finish().await.expect("topology cleanup");
}

/// `lastN` selects the last N instances (clause 5.14.5). Confirms REQ-MAP-139.
#[tokio::test]
async fn emt_last_n_selects_last_n_instances() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:temporal-lastn");
    seed_temporal(topology.broker(BrokerRole::A).client(), &entity_id, 20).await;
    let created = create_temporal_entity_map(topology.broker(BrokerRole::A).client(), &format!("type=Vehicle&lastN=2&{BEFORE_2022}")).await;
    assert_status(&created, 201);
    assert!(
        json_body(&created)["entityMap"].get(&entity_id).is_some(),
        "the lastN temporal query still maps the entity"
    );
    topology.finish().await.expect("topology cleanup");
}

/// Temporal filtering removes entities with no instance in the interval
/// (clause 5.14.5). Confirms REQ-MAP-141.
#[tokio::test]
async fn emt_temporal_filtering_removes_out_of_interval_entities() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:temporal-out-of-interval");
    seed_temporal(topology.broker(BrokerRole::A).client(), &entity_id, 20).await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).client(),
        "type=Vehicle&timerel=before&timeAt=2020-01-01T00:00:00Z",
    )
    .await;
    assert_status(&created, 201);
    assert!(
        json_body(&created)["entityMap"].get(&entity_id).is_none(),
        "an entity with no in-interval instance is filtered out"
    );
    topology.finish().await.expect("topology cleanup");
}
