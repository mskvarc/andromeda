//! Split-vs-non-split filter timing for entity queries (clause 5.7.2.4): a split
//! query strips value/geo/scope/attr filters before forwarding and re-applies them
//! after aggregation, while a non-split query applies the full filter locally and
//! forwards it to sources. broker-b holds `speed`, broker-c holds `location` for
//! the same entity id. Temporal split timing lives in `crate::cons::split_temporal`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// The `id` strings of an Entity Array body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// A rich value/geo/scope/attr filter set over `type=Vehicle`.
fn rich_filters() -> String {
    format!(
        "type=Vehicle&q={}&georel={}&geometry=Point&coordinates={}&scopeQ={}&attrs=speed",
        encode("speed>50"),
        encode("near;maxDistance==1000"),
        encode("[8.0,44.0]"),
        encode("/Madrid")
    )
}

/// A `GeoProperty` at a point inside the geofence used by [`rich_filters`].
fn location_node() -> Value {
    json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [8.0, 44.0] } })
}

/// Under `splitEntities` the local query stage matches on id/type/idPattern only,
/// so a split-holder whose local view lacks the filtered attribute is not
/// pre-excluded and survives to assembly (clause 5.7.2.4). Confirms REQ-CONS-21.
#[tokio::test]
async fn distop_split_entity_query_split_local_applies_id_type_pattern_only() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons21");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("fuelLevel", property(json!(70)))]))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b.client().post_entity(&vehicle(&entity_id, 60)).with_ld_json().send().await.expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("location", location_node())]))
            .with_ld_json()
            .send()
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons21-{role:?}"));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, peer.peer_url())
                        .mode("inclusive")
                        .operations(&["queryEntity"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
    }

    let response = front
        .client()
        .query_entities("type=Vehicle&splitEntities=true&q=speed>50", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        ids(&json_body(&response)).contains(&entity_id),
        "the split-holder survives the local stage and is assembled with the remote speed"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A non-split query applies the full filter set locally, so a local-only candidate
/// lacking the filtered attribute is filtered out locally (clause 5.7.2.4).
/// Confirms REQ-CONS-22.
#[tokio::test]
async fn distop_split_entity_query_non_split_applies_full_local_filter() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons22");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");

    assert_status(
        &front
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("fuelLevel", property(json!(70)))]))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );

    let response = front
        .client()
        .query_entities("type=Vehicle&local=true&splitEntities=false&q=speed>50", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        !ids(&json_body(&response)).contains(&entity_id),
        "the local-only candidate lacking speed is filtered out by the full local filter"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A split query strips value/geo/scope/attr filters before forwarding, leaving
/// only id/type on the forwarded request (clause 5.7.2.4). Confirms REQ-CONS-27.
#[tokio::test]
async fn distop_split_entity_query_split_strips_filters_before_forward() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    for source in ["/sb", "/sc"] {
        let registration_id = topology.id(&format!("registration:cons27-{}", source.trim_start_matches('/')));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, &mock.url(source))
                        .mode("inclusive")
                        .operations(&["queryEntity"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities"), 200, &json!([]))
            .await;
    }

    assert_status(
        &front
            .client()
            .query_entities(&format!("{}&splitEntities=true", rich_filters()), "application/ld+json", None)
            .await
            .expect("query"),
        200,
    );

    for _ in 0..2 {
        let forwarded = mock.next_request(Duration::from_secs(3)).await;
        let query = forwarded.uri.query().unwrap_or_default();
        assert!(query.contains("type"), "the forwarded request keeps the type filter, got {query:?}");
        assert!(!query.contains("q="), "the value filter is stripped, got {query:?}");
        assert!(
            !query.contains("georel") && !query.contains("scopeQ") && !query.contains("attrs="),
            "geo/scope/attr filters are stripped, got {query:?}"
        );
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A split query re-applies the stripped filters after aggregation: an entity
/// assembled to `speed>50` is returned while one below the threshold is excluded
/// (clause 5.7.2.4). Confirms REQ-CONS-28.
#[tokio::test]
async fn distop_split_entity_query_split_reapplies_filters_post_aggregation() {
    let topology = topology::chain().await.expect("three-broker topology");
    let matching_id = topology.id("Vehicle:cons28-match");
    let below_id = topology.id("Vehicle:cons28-below");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &holder_b
            .client()
            .post_entity(&vehicle(&matching_id, 60))
            .with_ld_json()
            .send()
            .await
            .expect("b match"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .post_entity(&vehicle(&below_id, 40))
            .with_ld_json()
            .send()
            .await
            .expect("b below"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&entity(&matching_id, "Vehicle", &[("location", location_node())]))
            .with_ld_json()
            .send()
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons28-{role:?}"));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, peer.peer_url())
                        .mode("inclusive")
                        .operations(&["queryEntity"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
    }

    let response = front
        .client()
        .query_entities("type=Vehicle&splitEntities=true&q=speed>50", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let returned = ids(&json_body(&response));
    assert!(
        returned.contains(&matching_id),
        "the assembled entity passing the re-applied filter is returned: {returned:?}"
    );
    assert!(!returned.contains(&below_id), "the entity below the threshold is excluded: {returned:?}");

    topology.finish().await.expect("topology cleanup");
}

/// A split non-local query re-checks value/geo/scope/attr filters on the aggregated
/// entities and removes one whose merged value fails (clause 5.7.2.4). Confirms
/// REQ-CONS-33.
#[tokio::test]
async fn distop_split_entity_query_split_nonlocal_removes_non_matching_after_recheck() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons33");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("fuelLevel", property(json!(70)))]))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b.client().post_entity(&vehicle(&entity_id, 40)).with_ld_json().send().await.expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("location", location_node())]))
            .with_ld_json()
            .send()
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons33-{role:?}"));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, peer.peer_url())
                        .mode("inclusive")
                        .operations(&["queryEntity"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
    }

    let response = front
        .client()
        .query_entities("type=Vehicle&splitEntities=true&q=speed>50", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(
        !ids(&json_body(&response)).contains(&entity_id),
        "the aggregated entity whose merged speed is 40 fails the re-check and is removed"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A non-split query forwards the value/geo/scope/attr filters to each source for
/// remote application (clause 5.7.2.4). Confirms REQ-CONS-22 remote-apply half and
/// reinforces REQ-CONS-27 by contrast.
#[tokio::test]
async fn distop_split_entity_query_non_split_forwards_filters_remotely() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    for source in ["/sb", "/sc"] {
        let registration_id = topology.id(&format!("registration:cons22r-{}", source.trim_start_matches('/')));
        assert_status(
            &front
                .client()
                .create_registration(
                    &CsrBuilder::new(&registration_id, &mock.url(source))
                        .mode("inclusive")
                        .operations(&["queryEntity"])
                        .build(),
                )
                .await
                .expect("register"),
            201,
        );
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities"), 200, &json!([]))
            .await;
    }

    assert_status(
        &front
            .client()
            .query_entities(&format!("{}&splitEntities=false", rich_filters()), "application/ld+json", None)
            .await
            .expect("query"),
        200,
    );

    for _ in 0..2 {
        let forwarded = mock.next_request(Duration::from_secs(3)).await;
        let query = forwarded.uri.query().unwrap_or_default();
        assert!(query.contains("q="), "the value filter is forwarded for remote application, got {query:?}");
        assert!(
            query.contains("georel") && query.contains("scopeQ") && query.contains("attrs="),
            "geo/scope/attr filters are forwarded, got {query:?}"
        );
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
