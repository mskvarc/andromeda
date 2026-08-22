//! Temporal upsert and add-attributes forwarding — REQ-PROV-78..88
//! (clauses 5.6.11.4, 5.6.12.4).
//!
//! An exclusive/redirect temporal registration owns `speed` and forwards its
//! instances while stripping them from the local evolution; an unadvertised
//! upsert yields `409 Conflict`; inclusive forwards a copy and writes locally;
//! and add-attributes only reports `404` when no registration matches. The
//! advertised operations are the Table 4.20-1 temporal names `upsertTemporal`
//! (5.6.11) and `appendAttrsTemporal` (5.6.12).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    oracle::{assert_problem_details, assert_status, json_body},
    prov::{
        forwarding::{FORWARD_WAIT, attr_present},
        temporal_payloads::{temporal_speed_fragment, temporal_vehicle},
    },
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{CONFLICT, RESOURCE_NOT_FOUND},
};
use axum::http::Method;
use serde_json::{Value, json};

/// A temporal `Vehicle` holding only a `speed` instance, for the wholly-owned
/// upsert that a registration must reject when it lacks `upsertTemporal`.
fn temporal_speed_only(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [ { "type": "Property", "value": 42, "observedAt": "2021-01-01T00:00:00Z" } ],
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

/// Exclusive temporal upsert forwards the owned `speed` instances and strips
/// them from the local evolution, keeping `brandName` local. Confirms
/// REQ-PROV-78, 79, 81.
#[tokio::test]
async fn temporal_upsert_exclusive_match_forwards_and_strips() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tupsert-exclusive");
    let reg_id = topology.id("registration:prov-tupsert-exclusive");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["upsertTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/temporal/entities", 201).await;

    let created = client.create_temporal_entity(&temporal_vehicle(&entity_id)).await.expect("temporal upsert");
    assert!(
        matches!(created.status.as_u16(), 200 | 201 | 204 | 207),
        "temporal upsert succeeds: {}",
        String::from_utf8_lossy(&created.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded temporal body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the exclusive attribute is forwarded: {forwarded_body}");
    assert!(
        !attr_present(&forwarded_body, "brandName"),
        "the unowned attribute is not forwarded: {forwarded_body}"
    );

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert!(local_body.get("brandName").is_some(), "the unowned attribute stays local: {local_body}");
    assert!(local_body.get("speed").is_none(), "the exclusive attribute is stripped locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive temporal registration without `upsertTemporal` rejects the owned
/// upsert with a bare `409 Conflict` and no forwarding. Confirms REQ-PROV-80.
#[tokio::test]
async fn temporal_upsert_exclusive_create_unsupported_returns_409() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tupsert-unsup");
    let reg_id = topology.id("registration:prov-tupsert-unsup");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let created = client.create_temporal_entity(&temporal_speed_only(&entity_id)).await.expect("temporal upsert");
    assert_status(&created, 409);
    assert_problem_details(&created, CONFLICT);
    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/temporal/entities").await,
        0,
        "no forward on an unsupported operation"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect temporal registration forwards the whole evolution and holds
/// nothing locally. Confirms REQ-PROV-82, 83.
#[tokio::test]
async fn temporal_upsert_redirect_match_forwards_and_strips() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tupsert-redirect");
    let reg_id = topology.id("registration:prov-tupsert-redirect");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["upsertTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/temporal/entities", 201).await;

    let created = client.create_temporal_entity(&temporal_vehicle(&entity_id)).await.expect("temporal upsert");
    assert!(
        matches!(created.status.as_u16(), 200 | 201 | 204 | 207),
        "temporal upsert succeeds: {}",
        String::from_utf8_lossy(&created.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded temporal body is JSON");
    assert!(
        attr_present(&forwarded_body, "speed"),
        "redirect forwards the whole evolution: {forwarded_body}"
    );
    assert!(
        attr_present(&forwarded_body, "brandName"),
        "redirect forwards the whole evolution: {forwarded_body}"
    );

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive temporal registration forwards a full copy and also writes the
/// evolution locally. Confirms REQ-PROV-84.
#[tokio::test]
async fn temporal_upsert_inclusive_forwards_remainder_and_local() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tupsert-inclusive");
    let reg_id = topology.id("registration:prov-tupsert-inclusive");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["upsertTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/temporal/entities", 201).await;

    let created = client.create_temporal_entity(&temporal_vehicle(&entity_id)).await.expect("temporal upsert");
    assert!(
        matches!(created.status.as_u16(), 200 | 201 | 204 | 207),
        "temporal upsert succeeds: {}",
        String::from_utf8_lossy(&created.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded temporal body is JSON");
    assert!(
        attr_present(&forwarded_body, "speed"),
        "inclusive forwards the full evolution: {forwarded_body}"
    );

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("speed").is_some(), "inclusive keeps the evolution locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Adding temporal attributes to an absent entity with no matching registration
/// is `404 ResourceNotFound`. Confirms REQ-PROV-85 (positive).
#[tokio::test]
async fn temporal_add_attrs_absent_and_no_registration_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let entity_id = topology.id("Vehicle:prov-tadd-absent");

    let response = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs"))
        .header("content-type", "application/ld+json")
        .json(&temporal_speed_fragment())
        .send()
        .await
        .expect("add temporal attrs");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching exclusive registration makes add-attributes forward the owned
/// `speed` even with no local entity, so the result is not `404`. Confirms
/// REQ-PROV-85 (negative), 86, 87.
#[tokio::test]
async fn temporal_add_attrs_registration_matches_absent_not_404_and_strips() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tadd-forward");
    let reg_id = topology.id("registration:prov-tadd-forward");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["appendAttrsTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let attrs_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &attrs_path, 204).await;

    let response = client
        .raw(Method::POST, &attrs_path)
        .header("content-type", "application/ld+json")
        .json(&temporal_speed_fragment())
        .send()
        .await
        .expect("add temporal attrs");
    assert!(
        response.status.as_u16() != 404,
        "a matching registration forwards rather than 404: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded temporal body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the owned attribute is forwarded: {forwarded_body}");
    assert_eq!(mock.get_count(Method::POST, &attrs_path).await, 1, "the add is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards the add-attributes remainder and applies
/// it locally. Confirms REQ-PROV-88.
#[tokio::test]
async fn temporal_add_attrs_inclusive_forwards_remainder_and_local() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tadd-inclusive");
    let reg_id = topology.id("registration:prov-tadd-inclusive");

    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["appendAttrsTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let attrs_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &attrs_path, 204).await;

    let response = client
        .raw(Method::POST, &attrs_path)
        .header("content-type", "application/ld+json")
        .json(&temporal_speed_fragment())
        .send()
        .await
        .expect("add temporal attrs");
    assert!(
        response.status.as_u16() != 404,
        "inclusive add succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::POST, &attrs_path).await, 1, "the add is forwarded once");

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("speed").is_some(), "the evolution keeps speed locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
