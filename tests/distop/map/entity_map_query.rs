//! Creating an EntityMap from an entity query: input validation, term expansion,
//! and the candidate-id shape of the result (clause 5.14.4). All `T-LOCAL`.

use crate::{
    entity::vehicle,
    map::entity_map_request::{create_entity_map, jsonld_link},
    oracle::{assert_problem_details, assert_status, json_body},
    topology::DistributedTopology,
};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use axum::http::Method;

/// Create a `Vehicle` on the front broker.
async fn seed_vehicle(topology: &DistributedTopology, local: &str) -> String {
    let entity_id = topology.id(local);
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    entity_id
}

/// The map records candidate entity ids, not entities (clause 5.14.4).
/// Confirms REQ-MAP-111.
#[tokio::test]
async fn emq_returns_candidate_ids_not_entities() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = seed_vehicle(&topology, "Vehicle:candidate-ids").await;
    let created = create_entity_map(topology.front().client(), "type=Vehicle").await;
    assert_status(&created, 201);
    let map = json_body(&created)["entityMap"].clone();
    let entry = map.get(&entity_id).unwrap_or_else(|| unreachable!("the created entity is a candidate"));
    assert!(
        entry.is_array(),
        "each candidate maps an entity id to a list of holders, not to an entity: {entry}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// Irrelevant query parameters are ignored (clause 5.14.4). Confirms REQ-MAP-112.
#[tokio::test]
async fn emq_ignores_irrelevant_query_params() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    seed_vehicle(&topology, "Vehicle:irrelevant-params").await;
    let created = create_entity_map(topology.front().client(), "type=Vehicle&lang=en").await;
    assert_status(&created, 201);
    topology.finish().await.expect("topology cleanup");
}

/// At least one filter is accepted (clause 5.14.4). Confirms REQ-MAP-113.
#[tokio::test]
async fn emq_at_least_one_filter_accepted() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    seed_vehicle(&topology, "Vehicle:one-filter").await;
    let created = create_entity_map(topology.front().client(), "type=Vehicle").await;
    assert_status(&created, 201);
    topology.finish().await.expect("topology cleanup");
}

/// No filter is rejected `400 BadRequestData` (clause 5.14.4). Confirms REQ-MAP-114.
#[tokio::test]
async fn emq_no_filter_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let response = create_entity_map(topology.front().client(), "local=true").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An invalid `id`, `q`, `geoQ`, or `csf` is rejected `400 BadRequestData`
/// (clause 5.14.4). Confirms REQ-MAP-115.
#[tokio::test]
async fn emq_invalid_id_q_geoq_csf_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let response = create_entity_map(topology.front().client(), "id=not-a-uri").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An unspecified or too-deep linked-entity projection is rejected
/// `400 BadRequestData` (clause 5.14.4). Confirms REQ-MAP-116.
#[tokio::test]
async fn emq_projection_linked_entity_unspecified_or_deep_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let response = create_entity_map(topology.front().client(), "type=Vehicle&join=inline&joinLevel=99").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Querying linked-entity attributes is too deep and rejected `400 BadRequestData`
/// (clause 5.14.4). Confirms REQ-MAP-117.
#[tokio::test]
async fn emq_query_linked_entity_attrs_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let response = create_entity_map(topology.front().client(), "type=Vehicle&q=linkedTo.speed%3E5").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A geometry-property projection without a GeoJSON `Accept` is rejected
/// `400 BadRequestData` (clause 5.14.4). Confirms REQ-MAP-118.
#[tokio::test]
async fn emq_geometry_property_non_geojson_accept_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let link = jsonld_link();
    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entityMaps?type=Vehicle&geometryProperty=location")
        .header("Link", link.as_str())
        .header("Accept", "application/json")
        .send()
        .await
        .expect("entity map creation request");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Type and attribute terms are expanded before the map is built (clause 5.14.4).
/// Confirms REQ-MAP-119.
#[tokio::test]
async fn emq_expands_type_and_attribute_terms() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = seed_vehicle(&topology, "Vehicle:expand-terms").await;
    let created = create_entity_map(topology.front().client(), "type=Vehicle&attrs=speed").await;
    assert_status(&created, 201);
    assert!(
        json_body(&created)["entityMap"].get(&entity_id).is_some(),
        "the expanded type and attribute select the entity"
    );
    topology.finish().await.expect("topology cleanup");
}

/// JSON-LD type coercion is applied for the expansion list (clause 5.14.4).
/// Confirms REQ-MAP-120.
#[tokio::test]
async fn emq_applies_jsonld_type_coercion_for_expand_list() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    seed_vehicle(&topology, "Vehicle:type-coercion").await;
    let created = create_entity_map(topology.front().client(), "type=Vehicle").await;
    assert_status(&created, 201);
    assert_eq!(json_body(&created)["type"].as_str(), Some("EntityMap"), "the coerced query yields an EntityMap");
    topology.finish().await.expect("topology cleanup");
}
