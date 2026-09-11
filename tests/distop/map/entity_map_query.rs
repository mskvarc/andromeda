//! Creating an `EntityMap` from an entity query: input validation, term expansion,
//! and the candidate-id shape of the result (clause 5.14.4). All `T-LOCAL`.

use crate::{
    entity::vehicle,
    map::entity_map_request::{create_entity_map, jsonld_link},
    oracle::{assert_problem_details, assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;

/// Create a `Vehicle` on the front broker.
async fn seed_vehicle(topology: &MultiBrokerTopology<BrokerRole>, local: &str) -> String {
    let entity_id = topology.id(local);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
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
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = seed_vehicle(&topology, "Vehicle:candidate-ids").await;
    let created = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "type=Vehicle").await;
    assert_status(&created, 201);
    let map = json_body(&created)["entityMap"].clone();
    let entry = map.get(&entity_id).unwrap_or_else(|| panic!("the created entity is a candidate"));
    assert!(
        entry.is_array(),
        "each candidate maps an entity id to a list of holders, not to an entity: {entry}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// Irrelevant query parameters are ignored (clause 5.14.4). Confirms REQ-MAP-112.
///
/// "Ignored" is a claim about the candidate set, not about acceptance: a `201`
/// alone would also be answered by a broker that let `lang` narrow the query.
/// The seeded entity therefore has to still be a candidate.
#[tokio::test]
async fn emq_ignores_irrelevant_query_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = seed_vehicle(&topology, "Vehicle:irrelevant-params").await;
    let created = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "type=Vehicle&lang=en").await;
    assert_status(&created, 201);
    let map = json_body(&created)["entityMap"].clone();
    assert!(
        map.get(&entity_id).is_some(),
        "the irrelevant lang parameter left the candidate set alone: {map}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// At least one filter is accepted (clause 5.14.4). Confirms REQ-MAP-113.
#[tokio::test]
async fn emq_at_least_one_filter_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    seed_vehicle(&topology, "Vehicle:one-filter").await;
    let created = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "type=Vehicle").await;
    assert_status(&created, 201);
    topology.finish().await.expect("topology cleanup");
}

/// No filter is rejected `400 BadRequestData` (clause 5.14.4). Confirms REQ-MAP-114.
#[tokio::test]
async fn emq_no_filter_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "local=true").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An invalid `id`, `q`, `geoQ`, or `csf` is rejected `400 BadRequestData`
/// (clause 5.14.4). Confirms REQ-MAP-115.
#[tokio::test]
async fn emq_invalid_id_q_geoq_csf_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "id=not-a-uri").await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A linked-entity projection with Linked Entity retrieval unspecified is
/// rejected `400 BadRequestData` (clause 5.14.4.4: "If projection attributes
/// are present and indicate the use of Linked Entity retrieval, and the use of
/// Linked Entity retrieval is not specified, or the projected attribute depth
/// exceeds the Linked Entity retrieval depth, then an error of type
/// `BadRequestData` shall be raised"). The projection uses the clause 4.21
/// `LinkedEntityTerm` form `linkedTo{speed}` and omits `join`, which clause
/// 5.14.4.3 lists among the accepted Query Entities parameters. Confirms
/// REQ-MAP-116.
#[tokio::test]
async fn emq_projection_linked_entity_unspecified_or_deep_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        "type=Vehicle&pick=linkedTo%7Bspeed%7D",
    )
    .await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A `q` filter over linked-entity attributes without Linked Entity retrieval is
/// rejected `400 BadRequestData` (clause 5.14.4.4: "If the filter conditions
/// specified by the query includes Linked Entity attributes, and the use of
/// Linked Entity retrieval is not specified ... an error of type `BadRequestData`
/// shall be raised (too deep query)"). The hop uses the clause 4.9 brace form
/// `linkedTo{speed>5}`; a dotted path would address a sub-attribute, not a
/// linked entity. Confirms REQ-MAP-117.
#[tokio::test]
async fn emq_query_linked_entity_attrs_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        "type=Vehicle&q=linkedTo%7Bspeed%3E5%7D",
    )
    .await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A geometry-property projection without a `GeoJSON` `Accept` is rejected
/// `400 BadRequestData` (clause 5.14.4). Confirms REQ-MAP-118.
#[tokio::test]
async fn emq_geometry_property_non_geojson_accept_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let link = jsonld_link();
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
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
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = seed_vehicle(&topology, "Vehicle:expand-terms").await;
    let created = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "type=Vehicle&attrs=speed").await;
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
    let topology = topology::single().await.expect("single-broker topology");
    seed_vehicle(&topology, "Vehicle:type-coercion").await;
    let created = create_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), "type=Vehicle").await;
    assert_status(&created, 201);
    assert_eq!(json_body(&created)["type"].as_str(), Some("EntityMap"), "the coerced query yields an EntityMap");
    topology.finish().await.expect("topology cleanup");
}
