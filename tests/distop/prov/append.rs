//! Append Attributes forwarding — REQ-PROV-20..26 (clause 5.6.3.4).
//!
//! An append to `POST /entities/{id}/attrs` returns `404` only when no local
//! entity and no matching registration exists; an exclusive registration
//! forwards its owned `speed` and appends the remainder locally; an unadvertised
//! `appendAttrs` yields `409 Conflict` bare or keyed inside `207 Multi-Status`;
//! inclusive forwards the full input and also appends locally; and a `T-PAIR`
//! row proves an inclusive append reaches both brokers.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{CONFLICT, RESOURCE_NOT_FOUND};
use axum::http::Method;
use serde_json::{Value, json};

/// A `Vehicle` carrying only the unowned `brandName`, seeded locally.
fn brand_only(id: &str) -> Value {
    entity(id, "Vehicle", &[("brandName", property(json!("Mercedes")))])
}

/// An append on an absent entity with no matching registration is `404
/// ResourceNotFound`. Confirms REQ-PROV-20 (positive).
#[tokio::test]
async fn append_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-append-absent");

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = client.append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching registration means an absent local entity does not force `404`:
/// the append forwards once. Confirms REQ-PROV-20 (negative).
#[tokio::test]
async fn append_registration_matches_entity_absent_not_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-append-match-absent");
    let reg_id = topology.id("registration:prov-append-match-absent");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["appendAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, &attrs_path, 204).await;

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = client.append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert!(
        response.status.as_u16() != 404,
        "a matching registration prevents 404: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::POST, &attrs_path), 1, "the append forwards once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive append forwards the owned `speed`, strips it from the local
/// fragment, and appends the remainder locally. Confirms REQ-PROV-21, 22, 24.
#[tokio::test]
async fn append_exclusive_match_forwards_and_strips() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-append-exclusive");
    let reg_id = topology.id("registration:prov-append-exclusive");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_only(&entity_id))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["appendAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, &attrs_path, 204).await;

    let fragment = attribute_fragment(&[("speed", property(json!(99))), ("colour", property(json!("red")))]);
    let response = client.append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert!(
        response.status.as_u16() != 404,
        "the append is accepted: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded append body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the exclusive attribute is forwarded: {forwarded_body}");
    assert!(
        !attr_present(&forwarded_body, "colour"),
        "the unowned attribute is not forwarded: {forwarded_body}"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert!(local_body.get("colour").is_some(), "the unowned attribute is appended locally: {local_body}");
    assert!(local_body.get("speed").is_none(), "the exclusive attribute is not stored locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration without `appendAttrs` rejects an owned-only append
/// with a bare `409 Conflict` and no forwarding. Confirms REQ-PROV-23
/// (complete-fail).
#[tokio::test]
async fn append_exclusive_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-append-unsup");
    let reg_id = topology.id("registration:prov-append-unsup");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_only(&entity_id))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = client.append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert_status(&response, 409);
    assert_problem_details(&response, CONFLICT);
    assert_eq!(mock.get_count(Method::POST, &attrs_path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The same unsupported `appendAttrs`, but with a local remainder, degrades to
/// `207 Multi-Status` carrying the `409` keyed by `registrationId`; the
/// remainder is appended locally. Confirms REQ-PROV-23 (partial), 26.
#[tokio::test]
async fn append_op_unsupported_with_remainder_returns_207() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-append-remainder");
    let reg_id = topology.id("registration:prov-append-remainder");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_only(&entity_id))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let fragment = attribute_fragment(&[("speed", property(json!(99))), ("colour", property(json!("red")))]);
    let response = client.append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert_eq!(errors[0]["registrationId"], json!(reg_id), "the conflict is keyed by registrationId: {body}");
    assert_eq!(errors[0]["error"]["status"], json!(409), "the unsupported operation surfaces a 409: {body}");
    assert_eq!(mock.get_count(Method::POST, &attrs_path), 0, "no forward on an unsupported operation");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("colour").is_some(), "the remainder is appended locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive append forwards the full input and also appends locally.
/// Confirms REQ-PROV-25, 26.
#[tokio::test]
async fn append_inclusive_forwards_remainder_and_appends_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-append-inclusive");
    let reg_id = topology.id("registration:prov-append-inclusive");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_only(&entity_id))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["appendAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, &attrs_path, 204).await;

    let fragment = attribute_fragment(&[("speed", property(json!(99))), ("colour", property(json!("red")))]);
    let response = client.append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert!(
        response.status.as_u16() != 404,
        "the append is accepted: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded append body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "inclusive forwards the full input: {forwarded_body}");
    assert!(attr_present(&forwarded_body, "colour"), "inclusive forwards the full input: {forwarded_body}");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert!(local_body.get("brandName").is_some(), "the pre-existing attribute remains: {local_body}");
    assert!(local_body.get("speed").is_some(), "the owned attribute is appended locally: {local_body}");
    assert!(local_body.get("colour").is_some(), "the remainder is appended locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// End-to-end against real broker B: an inclusive append reaches both the local
/// broker A copy and the peer broker B. Confirms REQ-PROV-25, 26.
#[tokio::test]
async fn append_pair_inclusive_end_to_end() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let entity_id = topology.id("Vehicle:prov-append-pair");
    let reg_id = topology.id("registration:prov-append-pair");

    let created = peer
        .client()
        .post_entity(&brand_only(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("peer entity create");
    assert_status(&created, 201);

    let seeded = front
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_only(&entity_id))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, peer.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["appendAttrs", "retrieveEntity", "queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = front.client().append_entity_attrs(&entity_id, &fragment).await.expect("append attributes");
    assert!(
        response.status.as_u16() != 404,
        "the append is accepted: {}",
        String::from_utf8_lossy(&response.body)
    );

    let front_local = front
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("front local read");
    assert_status(&front_local, 200);
    assert!(json_body(&front_local).get("speed").is_some(), "the append reaches broker A locally");

    let peer_local = peer
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("peer local read");
    assert_status(&peer_local, 200);
    assert!(json_body(&peer_local).get("speed").is_some(), "the append reaches broker B");

    topology.finish().await.expect("topology cleanup");
}
