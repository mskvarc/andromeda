//! Merge Entity forwarding — REQ-PROV-108..112 (clause 5.6.17.4).
//!
//! A merge with no local entity and no matching registration is `404`, but a
//! matching registration makes it forwardable rather than not-found. An
//! exclusive registration forwards the owned `speed` and strips it from the
//! local merge; an unadvertised `mergeEntity` yields a bare `409 Conflict`; and
//! an inclusive registration forwards the remainder and merges locally too.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::problem_detail::{CONFLICT, RESOURCE_NOT_FOUND};
use axum::http::Method;
use serde_json::{Value, json};

/// A `Vehicle` carrying a `brandName` and a `speed`.
fn brand_and_speed(id: &str) -> Value {
    entity(id, "Vehicle", &[("brandName", property(json!("Mercedes"))), ("speed", property(json!(42)))])
}

/// A merge fragment updating both `speed` and `brandName`.
fn merge_fragment() -> Value {
    attribute_fragment(&[("speed", property(json!(99))), ("brandName", property(json!("BMW")))])
}

/// Seed an entity purely locally, bypassing forwarding.
async fn seed_local(client: &TestClient, payload: &Value) {
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(payload)
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);
}

/// A merge for an absent entity with no matching registration is
/// `404 ResourceNotFound`. Confirms REQ-PROV-108 (positive).
#[tokio::test]
async fn merge_entity_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let entity_id = topology.id("Vehicle:prov-merge-absent");

    let merged = client
        .merge_entity(&entity_id, &attribute_fragment(&[("speed", property(json!(99)))]), None)
        .await
        .expect("merge entity");
    assert_status(&merged, 404);
    assert_problem_details(&merged, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A merge for an absent entity that a registration matches is forwarded rather
/// than answered `404`. Confirms REQ-PROV-108 (negative).
#[tokio::test]
async fn merge_entity_registration_matches_entity_absent_not_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-merge-match-absent");
    let reg_id = topology.id("registration:prov-merge-match-absent");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["mergeEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PATCH, &forward_path, 204).await;

    let merged = client
        .merge_entity(&entity_id, &attribute_fragment(&[("speed", property(json!(99)))]), None)
        .await
        .expect("merge entity");
    assert!(
        merged.status.as_u16() != 404,
        "a matching registration forwards rather than 404: {}",
        String::from_utf8_lossy(&merged.body)
    );
    assert_eq!(mock.get_count(Method::PATCH, &forward_path), 1, "the merge is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration forwards the owned `speed`, strips it from the local
/// merge, and keeps the unowned `brandName`. Confirms REQ-PROV-109, 111.
#[tokio::test]
async fn merge_entity_exclusive_match_forwards_and_strips() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-merge-exclusive");
    let reg_id = topology.id("registration:prov-merge-exclusive");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["mergeEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &brand_and_speed(&entity_id)).await;
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PATCH, &forward_path, 204).await;

    let merged = client.merge_entity(&entity_id, &merge_fragment(), None).await.expect("merge entity");
    assert!(merged.status.as_u16() != 404, "the merge succeeds: {}", String::from_utf8_lossy(&merged.body));

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded merge body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the owned attribute is forwarded: {forwarded_body}");
    assert!(
        !attr_present(&forwarded_body, "brandName"),
        "the unowned attribute is not forwarded: {forwarded_body}"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert_eq!(
        local_body["brandName"]["value"].as_str(),
        Some("BMW"),
        "the unowned attribute is merged locally: {local_body}"
    );
    assert_eq!(
        local_body["speed"]["value"].as_i64(),
        Some(42),
        "the owned attribute is removed from the fragment and the local copy is left untouched: {local_body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration that does not advertise `mergeEntity` rejects a
/// merge it wholly owns with a bare `409 Conflict` and no forwarding. Confirms
/// REQ-PROV-110.
#[tokio::test]
async fn merge_entity_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-merge-unsup");
    let reg_id = topology.id("registration:prov-merge-unsup");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &brand_and_speed(&entity_id)).await;
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");

    let merged = client
        .merge_entity(&entity_id, &attribute_fragment(&[("speed", property(json!(99)))]), None)
        .await
        .expect("merge entity");
    assert_status(&merged, 409);
    assert_problem_details(&merged, CONFLICT);
    assert_eq!(mock.get_count(Method::PATCH, &forward_path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards the remainder and also merges locally.
/// Confirms REQ-PROV-112.
#[tokio::test]
async fn merge_entity_inclusive_forwards_remainder_and_merges_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-merge-inclusive");
    let reg_id = topology.id("registration:prov-merge-inclusive");

    seed_local(client, &brand_and_speed(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["mergeEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PATCH, &forward_path, 204).await;

    let merged = client.merge_entity(&entity_id, &merge_fragment(), None).await.expect("merge entity");
    assert!(merged.status.as_u16() != 404, "the merge succeeds: {}", String::from_utf8_lossy(&merged.body));

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded merge body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "inclusive forwards the remainder: {forwarded_body}");
    assert!(attr_present(&forwarded_body, "brandName"), "inclusive forwards the remainder: {forwarded_body}");
    assert_eq!(mock.get_count(Method::PATCH, &forward_path), 1, "the merge is forwarded once");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert_eq!(
        local_body["speed"]["value"].as_i64(),
        Some(99),
        "the owned attribute is merged locally: {local_body}"
    );
    assert_eq!(
        local_body["brandName"]["value"].as_str(),
        Some("BMW"),
        "the remainder is merged locally: {local_body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
