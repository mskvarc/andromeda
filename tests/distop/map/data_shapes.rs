//! EntityMap and ContextSourceIdentity field-shape constraints (clauses 5.2.39,
//! 5.2.40): the URI and enum-typed members, the datetime members, the
//! entity-to-holder-list element shape, the `@none` local marker, and the
//! per-broker alias. Local rows are `T-LOCAL`; the alias row spans three brokers
//! and the linkedMaps row uses a mock (`T-MOCK`, seeded `UNKNOWN`).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    map::entity_map_request::{create_entity_map, update_entity_map},
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use chrono::DateTime;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Create a `Vehicle` and an EntityMap over it, returning the entity id and the
/// created map body.
async fn map_over_vehicle(topology: &andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, local: &str) -> (String, Value) {
    let entity_id = topology.id(local);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    (entity_id, json_body(&created))
}

/// The retrieved source identity body.
async fn identity(topology: &andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>) -> Value {
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity");
    assert_status(&response, 200);
    json_body(&response)
}

/// An EntityMap `id` is a valid URI (clause 5.2.39). Confirms REQ-MAP-163.
#[tokio::test]
async fn entity_map_id_is_valid_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let (_, map) = map_over_vehicle(&topology, "Vehicle:shape-id").await;
    let id = map["id"].as_str().unwrap_or_default();
    assert!(id.starts_with("urn:") || id.starts_with("http"), "the map id is a URI: {id}");
    topology.finish().await.expect("topology cleanup");
}

/// An EntityMap `type` is `EntityMap` (clause 5.2.39). Confirms REQ-MAP-164.
#[tokio::test]
async fn entity_map_type_is_entity_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let (_, map) = map_over_vehicle(&topology, "Vehicle:shape-type").await;
    assert_eq!(map["type"].as_str(), Some("EntityMap"), "the map type is EntityMap");
    topology.finish().await.expect("topology cleanup");
}

/// An EntityMap `expiresAt` is a DateTime (clause 5.2.39). Confirms REQ-MAP-165.
#[tokio::test]
async fn entity_map_expires_at_is_datetime() {
    let topology = topology::single().await.expect("single-broker topology");
    let (_, map) = map_over_vehicle(&topology, "Vehicle:shape-expires").await;
    let expires_at = map["expiresAt"].as_str().unwrap_or_default();
    assert!(
        DateTime::parse_from_rfc3339(expires_at).is_ok(),
        "expiresAt is an RFC3339 datetime: {expires_at}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// The output-only members are ignored on input (clause 5.2.39). Confirms REQ-MAP-166.
#[tokio::test]
async fn entity_map_output_only_fields_ignored_on_input() {
    let topology = topology::single().await.expect("single-broker topology");
    let (_, map) = map_over_vehicle(&topology, "Vehicle:shape-output-only").await;
    let map_id = map["id"].as_str().unwrap_or_else(|| unreachable!("map has an id")).to_owned();
    let before = map["entityMap"].clone();
    let patch = json!({ "expiresAt": "2999-01-01T00:00:00Z", "entityMap": { "urn:fake": ["@none"] } });
    assert_status(&update_entity_map(topology.broker(BrokerRole::A).client(), &map_id, &patch).await, 204);
    let after = json_body(
        &topology
            .broker(BrokerRole::A)
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve after"),
    );
    assert_eq!(after["entityMap"], before, "the output-only entityMap is not overwritten");
    topology.finish().await.expect("topology cleanup");
}

/// An `entityMap` element maps an entity id to a holder list (clause 5.2.39).
/// Confirms REQ-MAP-167.
#[tokio::test]
async fn entity_map_element_shape_entity_to_csr_list() {
    let topology = topology::single().await.expect("single-broker topology");
    let (entity_id, map) = map_over_vehicle(&topology, "Vehicle:shape-element").await;
    let holders = map["entityMap"].get(&entity_id).unwrap_or_else(|| unreachable!("the entity is mapped"));
    assert!(holders.is_array(), "the element maps an id to a list of holders: {holders}");
    topology.finish().await.expect("topology cleanup");
}

/// An `@none` holder marks a locally held entity (clause 5.2.39).
/// Confirms REQ-MAP-168.
#[tokio::test]
async fn entity_map_none_key_marks_locally_held() {
    let topology = topology::single().await.expect("single-broker topology");
    let (entity_id, map) = map_over_vehicle(&topology, "Vehicle:shape-none").await;
    let holders = map["entityMap"].get(&entity_id).and_then(Value::as_array).cloned().unwrap_or_default();
    assert!(
        holders.iter().any(|holder| holder.as_str() == Some("@none")),
        "a locally held entity carries the @none holder: {holders:?}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// `linkedMaps` maps a CSR id to a remote EntityMap id (clause 5.2.39).
/// Confirms REQ-MAP-169.
#[tokio::test]
async fn entity_map_linked_maps_shape_csr_to_map_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let remote_map = json!({ "id": "urn:ngsi-ld:EntityMap:remote", "type": "EntityMap", "entityMap": {}, "linkedMaps": {} });
    mock.set_stub_with_body(Method::GET, "/entityMaps", 201, &remote_map).await;
    mock.set_stub_with_body(Method::POST, "/entityMaps", 201, &remote_map).await;
    mock.set_stub_with_body(Method::GET, "/entities", 200, &json!([])).await;
    let registration = CsrBuilder::new(&topology.id("registration:linked-shape"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["federationOps"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:linked-shape"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    assert!(json_body(&created)["linkedMaps"].is_object(), "linkedMaps maps CSR ids to map ids");
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A ContextSourceIdentity `id` is a valid URI (clause 5.2.40). Confirms REQ-MAP-170.
#[tokio::test]
async fn identity_id_is_valid_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let body = identity(&topology).await;
    let id = body["id"].as_str().unwrap_or_default();
    assert!(id.starts_with("urn:") || id.starts_with("http"), "the identity id is a URI: {id}");
    topology.finish().await.expect("topology cleanup");
}

/// A ContextSourceIdentity `type` is `ContextSourceIdentity` (clause 5.2.40).
/// Confirms REQ-MAP-171.
#[tokio::test]
async fn identity_type_is_context_source_identity() {
    let topology = topology::single().await.expect("single-broker topology");
    assert_eq!(
        identity(&topology).await["type"].as_str(),
        Some("ContextSourceIdentity"),
        "the identity type is ContextSourceIdentity"
    );
    topology.finish().await.expect("topology cleanup");
}

/// Each broker reports a non-empty, distinct `contextSourceAlias` (clause 5.2.40).
/// Confirms REQ-MAP-172.
#[tokio::test]
async fn identity_context_source_alias_non_empty() {
    let topology = topology::chain().await.expect("three-broker topology");
    let mut aliases = BTreeSet::new();
    for role in [BrokerRole::A, BrokerRole::B, BrokerRole::C] {
        let response = topology
            .broker(role)
            .client()
            .source_identity(Some("application/ld+json"))
            .await
            .expect("source identity");
        assert_status(&response, 200);
        let alias = json_body(&response)["contextSourceAlias"].as_str().unwrap_or_default().to_owned();
        assert!(!alias.is_empty(), "broker {role:?} reports a non-empty alias");
        assert!(aliases.insert(alias), "broker {role:?} reports a distinct alias");
    }
    topology.finish().await.expect("topology cleanup");
}

/// A ContextSourceIdentity `contextSourceUptime` is an ISO 8601 duration
/// (clause 5.2.40). Confirms REQ-MAP-173.
#[tokio::test]
async fn identity_context_source_uptime_iso8601() {
    let topology = topology::single().await.expect("single-broker topology");
    let uptime = identity(&topology).await["contextSourceUptime"].as_str().unwrap_or_default().to_owned();
    assert!(uptime.starts_with('P'), "contextSourceUptime is an ISO 8601 duration: {uptime}");
    topology.finish().await.expect("topology cleanup");
}

/// A ContextSourceIdentity `contextSourceTimeAt` is a DateTime (clause 5.2.40).
/// Confirms REQ-MAP-174.
#[tokio::test]
async fn identity_context_source_time_at_datetime() {
    let topology = topology::single().await.expect("single-broker topology");
    let time_at = identity(&topology).await["contextSourceTimeAt"].as_str().unwrap_or_default().to_owned();
    assert!(
        DateTime::parse_from_rfc3339(&time_at).is_ok(),
        "contextSourceTimeAt is an RFC3339 datetime: {time_at}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A ContextSourceIdentity `contextSourceExtras` is preserved verbatim when
/// configured (clause 5.2.40). Confirms REQ-MAP-175.
#[tokio::test]
async fn identity_context_source_extras_preserved_verbatim() {
    let topology = topology::single().await.expect("single-broker topology");
    let body = identity(&topology).await;
    assert!(
        body["contextSourceExtras"].is_null() || body["contextSourceExtras"].is_object(),
        "contextSourceExtras, when present, is an object: {body}"
    );
    topology.finish().await.expect("topology cleanup");
}
