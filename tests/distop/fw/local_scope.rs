//! Local scope: `local=true`, the type wildcard, and a `localOnly` subscription
//! or registration keep an operation to directly-held data and off the registry
//! (ETSI GS CIM 009 v1.9.1, clauses 4.3.6.3, 5.7, 5.11). `T-MOCK` for
//! no-forwarding checks, `T-PAIR` for the subscription. Snapshot locality
//! (clause 5.5.13) is asserted with the snapshot rows in `fw/snapshots.rs`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::typed_vehicle,
    notification_adapter::NotificationAdapter,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// Locate the entity with the given id inside an entity-array response body.
fn find_entity<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()?.iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// `local=true` restricts a query to directly-held data: no source is contacted
/// and only local entities are returned (clause 4.3.6.3). Confirms REQ-FW-66,
/// REQ-FW-69.
#[tokio::test]
async fn distop_entity_local_scope_true_uses_only_directly_held_data() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:local-true");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 12))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:local-true"), &endpoint)
        .mode("inclusive")
        .entity_type(&vtype)
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&local=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count(), 0, "local=true contacts no source");
    assert!(find_entity(&json_body(&response), &entity_id).is_some(), "only local entities are returned");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `type=*` wildcard query implies local scope: the registry is not consulted
/// (clause 4.3.6.3). Confirms REQ-FW-66, REQ-FW-69.
#[tokio::test]
async fn distop_entity_local_scope_type_wildcard_implies_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:local-wildcard");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 12))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:local-wildcard"), &endpoint)
        .mode("inclusive")
        .entity_type(&vtype)
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=*", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count(), 0, "a type wildcard consults no registry CSR");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With several registrations present, a `local=true` operation consults none of
/// them and returns only local data (clause 4.3.6.3). Confirms REQ-FW-69.
#[tokio::test]
async fn distop_entity_local_scope_true_skips_registry_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:local-skip");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 12))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    for suffix in ["a", "b"] {
        let registration = CsrBuilder::new(&topology.id(&format!("registration:local-skip-{suffix}")), &endpoint)
            .mode("inclusive")
            .entity_type(&vtype)
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
    }

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&local=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count(), 0, "no registration is consulted under local scope");
    assert!(find_entity(&json_body(&response), &entity_id).is_some(), "local data is returned");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `localOnly` subscription matches only local changes: a change to a local
/// entity notifies, while a remote-only entity does not (clause 5.11). Confirms
/// REQ-FW-67.
#[tokio::test]
async fn distop_entity_local_scope_subscription_local_only_matches_local_only() {
    let topology = topology::pair().await.expect("two-broker topology");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let receiver = NotificationAdapter::start().await;

    let subscription_id = topology.id("Subscription:local-only");
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [ { "type": vtype } ],
        "localOnly": true,
        "notification": { "endpoint": { "uri": receiver.endpoint_url(), "accept": "application/json" } },
        "@context": COMPOUND_CONTEXT.as_str()
    });
    assert_status(&front.client().create_subscription(&subscription).await.expect("create subscription"), 201);

    let local_entity = topology.id("Vehicle:local-only-change");
    assert_status(
        &front
            .client()
            .post_entity(&typed_vehicle(&local_entity, &vtype, 30))
            .with_ld_json()
            .send()
            .await
            .expect("local change"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(10)).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(subscription_id.as_str()),
        "the local change notifies"
    );

    receiver.clear_notifications().await;
    let remote_entity = topology.id("Vehicle:remote-only-change");
    assert_status(
        &peer
            .client()
            .post_entity(&typed_vehicle(&remote_entity, &vtype, 40))
            .with_ld_json()
            .send()
            .await
            .expect("remote change"),
        201,
    );
    receiver.wait_for_no_subscription_notification(&subscription_id, Duration::from_secs(3)).await;

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A registration whose `management.localOnly` is set forwards its query with a
/// `local=true` flag, so the source does not cascade further (clause 4.3.6.3).
/// Confirms REQ-FW-68.
#[tokio::test]
async fn distop_entity_local_scope_management_local_only_forwards_local_flag() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let vtype = topology.id("Vehicle");
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:management-localonly"), &endpoint)
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .management(json!({ "localOnly": true }))
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        forwarded_query.contains("local=true"),
        "a localOnly registration forwards the local flag: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
