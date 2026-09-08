//! Temporal delete-attribute and instance forwarding — REQ-PROV-89..103
//! (clauses 5.6.13.4, 5.6.14.4, 5.6.15.4). Exclusive registrations forward and
//! terminate, inclusive forward the input, an unadvertised operation is
//! `409 Conflict`, and `404` surfaces only when nothing matches. The advertised
//! operations are the Table 4.20-1 temporal names: `deleteAttrsTemporal`
//! (5.6.13), `updateAttrInstanceTemporal` (5.6.14) and
//! `deleteAttrInstanceTemporal` (5.6.15).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    oracle::{assert_problem_details, assert_status, json_body},
    prov::{
        forwarding::{FORWARD_WAIT, request_path},
        temporal_payloads::{speed_instance, temporal_vehicle},
    },
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{CONFLICT, RESOURCE_NOT_FOUND},
};
use axum::http::Method;
use serde_json::{Value, json};

/// A temporal `Vehicle` holding only a `brandName` instance, for the
/// present-entity/absent-attribute rows.
fn temporal_brand_only(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": [ { "type": "Property", "value": "Mercedes", "observedAt": "2021-01-01T00:00:00Z" } ],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// Seed a purely-local temporal entity, bypassing forwarding.
async fn seed_local_temporal(client: &TestClient, body: &Value) {
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/temporal/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("seed local temporal");
    assert!(
        matches!(seeded.status.as_u16(), 201 | 204),
        "local temporal seed succeeds: {}",
        String::from_utf8_lossy(&seeded.body)
    );
}

/// Deleting a temporal attribute on an absent entity with no matching
/// registration is `404 ResourceNotFound`. Confirms REQ-PROV-89 (positive).
#[tokio::test]
async fn temporal_delete_attr_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-tdelattr-absent");

    let response = client
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed"))
        .send()
        .await
        .expect("delete temporal attr");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// Exclusive delete-attribute forwards to the owning source and terminates local
/// processing, leaving the local instances intact. Confirms REQ-PROV-89
/// (negative), 90, 91.
#[tokio::test]
async fn temporal_delete_attr_exclusive_match_forwards_and_terminates() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tdelattr-excl");
    let reg_id = topology.id("registration:prov-tdelattr-excl");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["deleteAttrsTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;
    let attr_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &attr_path, 204).await;

    let response = client.raw(Method::DELETE, &attr_path).send().await.expect("delete temporal attr");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "exclusive delete-attr succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with("/attrs/speed"),
        "the delete forwards to the owning source: {}",
        request_path(&forwarded)
    );

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 200);
    assert!(
        json_body(&local).get("speed").is_some(),
        "exclusive delete terminates without touching local instances"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Inclusive delete-attribute forwards the input and removes the attribute
/// locally. Confirms REQ-PROV-92.
#[tokio::test]
async fn temporal_delete_attr_inclusive_forwards_input() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tdelattr-incl");
    let reg_id = topology.id("registration:prov-tdelattr-incl");

    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["deleteAttrsTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let attr_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &attr_path, 204).await;

    let response = client.raw(Method::DELETE, &attr_path).send().await.expect("delete temporal attr");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "inclusive delete-attr succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::DELETE, &attr_path), 1, "the delete is forwarded once");

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("speed").is_none(), "inclusive delete removes the attribute locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an absent attribute of a locally present temporal entity is
/// `404 ResourceNotFound`. Confirms REQ-PROV-93.
#[tokio::test]
async fn temporal_delete_attr_local_present_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-tdelattr-missing");

    seed_local_temporal(client, &temporal_brand_only(&entity_id)).await;

    let response = client
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed"))
        .send()
        .await
        .expect("delete temporal attr");
    assert_status(&response, 404);

    topology.finish().await.expect("topology cleanup");
}

/// Modifying an instance on an absent entity with no matching registration is
/// `404 ResourceNotFound`. Confirms REQ-PROV-94 (positive).
#[tokio::test]
async fn temporal_modify_instance_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-tmod-absent");
    let instance_id = topology.id("instance:1");

    let response = client
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}"))
        .header("content-type", "application/ld+json")
        .json(&speed_instance())
        .send()
        .await
        .expect("modify temporal instance");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// Exclusive modify-instance forwards to the owning source and terminates local
/// processing. Confirms REQ-PROV-94 (negative), 95, 97.
#[tokio::test]
async fn temporal_modify_instance_exclusive_supported_forwards_and_terminates() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tmod-excl");
    let reg_id = topology.id("registration:prov-tmod-excl");
    let instance_id = topology.id("instance:1");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["updateAttrInstanceTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;
    let instance_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}");
    mock.set_stub(Method::PATCH, &instance_path, 204).await;

    let response = client
        .raw(Method::PATCH, &instance_path)
        .header("content-type", "application/ld+json")
        .json(&speed_instance())
        .send()
        .await
        .expect("modify temporal instance");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "exclusive modify-instance succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with(instance_id.as_str()),
        "the modify forwards to the owning source: {}",
        request_path(&forwarded)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration without `updateAttrs` rejects a modify-instance
/// with `409 Conflict` and no forwarding. Confirms REQ-PROV-96.
#[tokio::test]
async fn temporal_modify_instance_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tmod-unsup");
    let reg_id = topology.id("registration:prov-tmod-unsup");
    let instance_id = topology.id("instance:1");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;
    let instance_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}");

    let response = client
        .raw(Method::PATCH, &instance_path)
        .header("content-type", "application/ld+json")
        .json(&speed_instance())
        .send()
        .await
        .expect("modify temporal instance");
    assert_status(&response, 409);
    assert_problem_details(&response, CONFLICT);
    assert_eq!(mock.get_count(Method::PATCH, &instance_path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Inclusive modify-instance forwards the input. Confirms REQ-PROV-98.
#[tokio::test]
async fn temporal_modify_instance_inclusive_forwards_input() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tmod-incl");
    let reg_id = topology.id("registration:prov-tmod-incl");
    let instance_id = topology.id("instance:1");

    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["updateAttrInstanceTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let instance_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}");
    mock.set_stub(Method::PATCH, &instance_path, 204).await;

    let response = client
        .raw(Method::PATCH, &instance_path)
        .header("content-type", "application/ld+json")
        .json(&speed_instance())
        .send()
        .await
        .expect("modify temporal instance");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "inclusive modify-instance succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::PATCH, &instance_path), 1, "the modify is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an instance on an absent entity with no matching registration is
/// `404 ResourceNotFound`. Confirms REQ-PROV-99 (positive).
#[tokio::test]
async fn temporal_delete_instance_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-tdelinst-absent");
    let instance_id = topology.id("instance:1");

    let response = client
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}"))
        .send()
        .await
        .expect("delete temporal instance");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// Exclusive delete-instance forwards to the owning source and terminates local
/// processing. Confirms REQ-PROV-99 (negative), 100, 101.
#[tokio::test]
async fn temporal_delete_instance_exclusive_match_forwards_and_terminates() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tdelinst-excl");
    let reg_id = topology.id("registration:prov-tdelinst-excl");
    let instance_id = topology.id("instance:1");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["deleteAttrInstanceTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;
    let instance_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}");
    mock.set_stub(Method::DELETE, &instance_path, 204).await;

    let response = client.raw(Method::DELETE, &instance_path).send().await.expect("delete temporal instance");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "exclusive delete-instance succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with(instance_id.as_str()),
        "the delete forwards to the owning source: {}",
        request_path(&forwarded)
    );

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 200);
    assert!(
        json_body(&local).get("speed").is_some(),
        "exclusive delete-instance terminates without touching local instances"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Inclusive delete-instance forwards the input. Confirms REQ-PROV-102.
#[tokio::test]
async fn temporal_delete_instance_inclusive_forwards_input() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tdelinst-incl");
    let reg_id = topology.id("registration:prov-tdelinst-incl");
    let instance_id = topology.id("instance:1");

    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["deleteAttrInstanceTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let instance_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}");
    mock.set_stub(Method::DELETE, &instance_path, 204).await;

    let response = client.raw(Method::DELETE, &instance_path).send().await.expect("delete temporal instance");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "inclusive delete-instance succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::DELETE, &instance_path), 1, "the delete is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an absent instance of a locally present temporal entity is
/// `404 ResourceNotFound`. Confirms REQ-PROV-103.
#[tokio::test]
async fn temporal_delete_instance_local_present_instance_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-tdelinst-missing");
    let absent_instance = topology.id("instance:absent");

    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;

    let response = client
        .raw(
            Method::DELETE,
            &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{absent_instance}"),
        )
        .send()
        .await
        .expect("delete temporal instance");
    assert_status(&response, 404);

    topology.finish().await.expect("topology cleanup");
}
