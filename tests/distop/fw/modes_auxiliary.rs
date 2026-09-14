//! Auxiliary mode: a source only supplements retrieval, never overriding a
//! local attribute and never receiving writes, and its registration is always
//! accepted even overlapping an exclusive one (ETSI GS CIM 009 v1.9.1, clauses
//! 4.3.6.2, 4.3.6.3, 5.7). Real-peer merge on `T-PAIR`; forwarding checks on
//! `T-MOCK`; registration acceptance on `T-LOCAL`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, entity, property, vehicle},
    oracle::{assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// An auxiliary source never overrides a local attribute: the local value wins
/// while a genuinely new source attribute is added (clause 4.3.6.2). Confirms
/// REQ-FW-15, REQ-FW-17.
#[tokio::test]
async fn distop_entity_auxiliary_never_overrides_local_attribute() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let entity_id = topology.id("Vehicle:auxiliary-merge");

    assert_status(
        &front
            .client()
            .post_entity(&vehicle(&entity_id, 50))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    let remote = entity(&entity_id, "Vehicle", &[("speed", property(json!(99))), ("fuel", property(json!(20)))]);
    assert_status(&peer.client().post_entity(&remote).with_ld_json().send().await.expect("peer create"), 201);

    let registration = CsrBuilder::new(&topology.id("registration:auxiliary-merge"), peer.peer_url())
        .mode("auxiliary")
        .entity_id(&entity_id)
        .property_names(&["speed", "fuel"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(
        body["speed"]["value"].as_i64(),
        Some(50),
        "the local attribute is not overridden by the auxiliary source"
    );
    assert_eq!(body["fuel"]["value"].as_i64(), Some(20), "the new auxiliary attribute is supplemented");

    topology.finish().await.expect("topology cleanup");
}

/// Only a genuinely supplementary attribute from an auxiliary source is
/// included: an attribute already held locally keeps its local value and the
/// source copy is discarded (clause 4.3.6.2). Confirms REQ-FW-17.
#[tokio::test]
async fn distop_entity_auxiliary_supplementary_attribute_only_included() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:auxiliary-supplement");
    let source_entity = entity(&entity_id, "Vehicle", &[("speed", property(json!(99))), ("fuel", property(json!(20)))]);
    mock.set_stub_with_body(Method::GET, &format!("{FORWARDED_ENTITIES}/{entity_id}"), 200, &source_entity)
        .await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 50))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:auxiliary-supplement"), &endpoint)
        .mode("auxiliary")
        .entity_id(&entity_id)
        .property_names(&["speed", "fuel"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(
        body["speed"]["value"].as_i64(),
        Some(50),
        "the local attribute keeps its value, the source copy is discarded"
    );
    assert_eq!(body["fuel"]["value"].as_i64(), Some(20), "only the supplementary attribute is added");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary registration never receives writes: an append is applied locally
/// only and nothing is forwarded to the source (clause 4.3.6.2). Confirms
/// REQ-FW-16.
#[tokio::test]
async fn distop_entity_auxiliary_write_not_forwarded() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:auxiliary-write");

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 10))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:auxiliary-write"), &endpoint)
        .mode("auxiliary")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );

    let fragment = attribute_fragment(&[("speed", property(json!(20)))]);
    let appended = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .append_entity_attrs(&entity_id, &fragment)
        .await
        .expect("append attrs");
    assert!(appended.status.is_success(), "the append succeeds locally: {}", appended.status);
    assert_eq!(mock.get_total_count(), 0, "an auxiliary registration never receives a write");

    let local = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local retrieve");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["speed"]["value"].as_i64(), Some(20), "the write is applied locally only");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary registration overlapping an existing exclusive one on the same
/// id and attribute is always accepted (clause 4.3.6.2, 5.2.9). Confirms
/// REQ-FW-18.
#[tokio::test]
async fn distop_entity_auxiliary_registration_overlapping_exclusive_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:auxiliary-overlap");

    let exclusive = CsrBuilder::new(&topology.id("registration:auxiliary-overlap-exclusive"), PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&exclusive)
            .await
            .expect("exclusive registration"),
        201,
    );

    let auxiliary = CsrBuilder::new(&topology.id("registration:auxiliary-overlap-aux"), PEER_ENDPOINT)
        .mode("auxiliary")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    let accepted = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&auxiliary)
        .await
        .expect("auxiliary registration");
    assert_status(&accepted, 201);

    topology.finish().await.expect("topology cleanup");
}
