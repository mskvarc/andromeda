//! Split-vs-non-split filter timing for temporal queries (clause 5.7.4.4/.5): a
//! split temporal query strips value/geo/scope filters before forwarding (keeping
//! the temporal query) and re-applies them after aggregation, while a non-split
//! query stages the full local filtering. broker-b holds `speed`, broker-c holds
//! `location` for the same entity id.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{BrokerRole, DistributedTopology},
};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// The `id` strings of a temporal array body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// The temporal entity with the given id from a temporal array body.
fn temporal_entity(body: &Value, id: &str) -> Value {
    body.as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(id)))
        .cloned()
        .unwrap_or(Value::Null)
}

/// A temporal `GeoProperty` history at a point inside the geofence.
fn location_history() -> Value {
    json!([{ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [8.0, 44.0] }, "observedAt": "2020-08-01T12:00:00Z" }])
}

const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// A rich value/geo/scope temporal filter set over `type=Vehicle`.
fn rich_temporal_filters() -> String {
    format!(
        "type=Vehicle&{BEFORE_2030}&q={}&georel={}&geometry=Point&coordinates={}&scopeQ={}",
        encode("speed>50"),
        encode("near;maxDistance==1000"),
        encode("[8.0,44.0]"),
        encode("/Madrid")
    )
}

/// Under `splitEntities` the local temporal stage produces S1 from id/idPattern/type
/// plus the temporal query only, so a split-holder survives to assembly
/// (clause 5.7.4.4). Confirms REQ-CONS-56.
#[tokio::test]
async fn distop_split_temporal_query_split_nonlocal_local_stage_id_type_temporal_only() {
    let topology = DistributedTopology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons56");
    let front = topology.front();
    let holder_b = topology.broker(BrokerRole::B);
    let holder_c = topology.broker(BrokerRole::C);

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("fuelLevel", json!([observed(70, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(60, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("location", location_history())]))
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role);
        let registration_id = topology.id(&format!("registration:cons56-{role:?}"));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, peer.peer_url())
                        .mode("inclusive")
                        .operations(&["queryTemporal"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
    }

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&splitEntities=true&q=speed>50"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        ids(&json_body(&response)).contains(&entity_id),
        "the split-holder survives the local temporal stage and is assembled"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A non-split temporal query stages the full local filtering, so a local candidate
/// failing the value stage is absent from the result (clause 5.7.4.4). Confirms
/// REQ-CONS-57.
#[tokio::test]
async fn distop_split_temporal_query_non_split_staged_local_filtering() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons57");
    let front = topology.front();

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("fuelLevel", json!([observed(70, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("local"),
        201,
    );

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&local=true&splitEntities=false&q=speed>50"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        !ids(&json_body(&response)).contains(&entity_id),
        "the local candidate failing the value stage is filtered out"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A split temporal query strips value/geo/scope filters before forwarding while
/// retaining the temporal query (clause 5.7.4.4). Confirms REQ-CONS-62.
#[tokio::test]
async fn distop_split_temporal_query_split_strips_then_reapplies() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let front = topology.front();
    let mock = ContextSourceAdapter::start().await;

    for source in ["/sb", "/sc"] {
        let registration_id = topology.id(&format!("registration:cons62-{}", source.trim_start_matches('/')));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, &mock.url(source))
                        .mode("inclusive")
                        .operations(&["queryTemporal"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/temporal/entities"), 200, &json!([]))
            .await;
    }

    assert_status(
        &front
            .client()
            .query_temporal_entities(&format!("{}&splitEntities=true", rich_temporal_filters()), None)
            .await
            .expect("query"),
        200,
    );

    for _ in 0..2 {
        let forwarded = mock.next_request(Duration::from_secs(3)).await;
        let query = forwarded.uri.query().unwrap_or_default();
        assert!(
            query.contains("timerel"),
            "the temporal query is retained on the forwarded request, got {query:?}"
        );
        assert!(!query.contains("q="), "the value filter is stripped, got {query:?}");
        assert!(
            !query.contains("georel") && !query.contains("scopeQ"),
            "geo/scope filters are stripped, got {query:?}"
        );
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A split non-local temporal query re-applies value/geo/scope on the aggregated
/// entities, removing one whose merged value fails (clause 5.7.4.4). Confirms
/// REQ-CONS-65.
#[tokio::test]
async fn distop_split_temporal_query_split_nonlocal_reapplies_values_geo_scope() {
    let topology = DistributedTopology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons65");
    let front = topology.front();
    let holder_b = topology.broker(BrokerRole::B);
    let holder_c = topology.broker(BrokerRole::C);

    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(40, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("location", location_history())]))
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role);
        let registration_id = topology.id(&format!("registration:cons65-{role:?}"));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, peer.peer_url())
                        .mode("inclusive")
                        .operations(&["queryTemporal"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
    }

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&splitEntities=true&q=speed>50"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        !ids(&json_body(&response)).contains(&entity_id),
        "the aggregated entity whose merged speed is 40 fails the re-applied value filter"
    );

    topology.finish().await.expect("topology cleanup");
}

/// When no split re-filtering applies, S7 equals S4: the aggregated temporal result
/// passes through unchanged (clause 5.7.4.4). Confirms REQ-CONS-66.
#[tokio::test]
async fn distop_split_temporal_query_no_reapply_s7_equals_s4() {
    let topology = DistributedTopology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons66");
    let front = topology.front();
    let holder_b = topology.broker(BrokerRole::B);
    let holder_c = topology.broker(BrokerRole::C);

    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(60, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("location", location_history())]))
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role);
        let registration_id = topology.id(&format!("registration:cons66-{role:?}"));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, peer.peer_url())
                        .mode("inclusive")
                        .operations(&["queryTemporal"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
    }

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let merged = temporal_entity(&json_body(&response), &entity_id);
    assert!(
        merged.get("speed").is_some() && merged.get("location").is_some(),
        "the aggregated temporal entity passes through unchanged: {merged}"
    );

    topology.finish().await.expect("topology cleanup");
}
