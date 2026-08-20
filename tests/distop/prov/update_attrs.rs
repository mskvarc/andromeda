//! Update Attributes forwarding — REQ-PROV-13..19 (clause 5.6.2.4).
//!
//! A partial update of `PATCH /entities/{id}/attrs` returns `404` only when no
//! local entity and no matching registration exists; an exclusive registration
//! forwards its owned `speed` and patches the remainder locally; an unadvertised
//! `updateAttrs` yields `409 Conflict` bare or keyed inside `207 Multi-Status`;
//! inclusive forwards the full remainder and also patches locally; and a
//! `T-PAIR` row proves the split end-to-end against real broker B.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present},
    registration::CsrBuilder,
    topology::{BrokerRole, DistributedTopology},
};
use andromeda_test_fixtures::problem_detail::{CONFLICT, RESOURCE_NOT_FOUND};
use axum::http::Method;
use serde_json::{Value, json};

/// A `Vehicle` carrying only the unowned `brandName`, seeded locally.
fn brand_only(id: &str) -> Value {
    entity(id, "Vehicle", &[("brandName", property(json!("Mercedes")))])
}

/// An update on an absent entity with no matching registration is `404
/// ResourceNotFound`. Confirms REQ-PROV-13 (positive).
#[tokio::test]
async fn update_attrs_absent_and_no_registration_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let entity_id = topology.id("Vehicle:prov-update-absent");

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = client.update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching registration means an absent local entity does not force `404`:
/// the update forwards once. Confirms REQ-PROV-13 (negative).
#[tokio::test]
async fn update_attrs_registration_matches_entity_absent_not_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-update-match-absent");
    let reg_id = topology.id("registration:prov-update-match-absent");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &attrs_path, 204).await;

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = client.update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert!(
        response.status.as_u16() != 404,
        "a matching registration prevents 404: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::PATCH, &attrs_path).await, 1, "the update forwards once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive update forwards the owned `speed`, strips it from the local
/// patch, and applies the remainder locally. Confirms REQ-PROV-14, 15, 17.
#[tokio::test]
async fn update_attrs_exclusive_match_forwards_and_strips() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-update-exclusive");
    let reg_id = topology.id("registration:prov-update-exclusive");
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
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &attrs_path, 204).await;

    let fragment = attribute_fragment(&[("speed", property(json!(99))), ("brandName", property(json!("BMW")))]);
    let response = client.update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert!(
        response.status.as_u16() != 404,
        "the update is accepted: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded update body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the exclusive attribute is forwarded: {forwarded_body}");
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
        "the remainder is patched locally: {local_body}"
    );
    assert!(local_body.get("speed").is_none(), "the exclusive attribute is not stored locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration without `updateAttrs` rejects an owned-only update
/// with a bare `409 Conflict` and no forwarding. Confirms REQ-PROV-16
/// (complete-fail).
#[tokio::test]
async fn update_attrs_exclusive_op_unsupported_returns_409() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-update-unsup");
    let reg_id = topology.id("registration:prov-update-unsup");
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
    let response = client.update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert_status(&response, 409);
    assert_problem_details(&response, CONFLICT);
    assert_eq!(mock.get_count(Method::PATCH, &attrs_path).await, 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The same unsupported `updateAttrs`, but with a local remainder, degrades to
/// `207 Multi-Status` carrying the `409` keyed by `registrationId`; the
/// remainder is patched locally. Confirms REQ-PROV-16 (partial), 19.
#[tokio::test]
async fn update_attrs_op_unsupported_with_remainder_returns_207() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-update-remainder");
    let reg_id = topology.id("registration:prov-update-remainder");
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

    let fragment = attribute_fragment(&[("speed", property(json!(99))), ("brandName", property(json!("BMW")))]);
    let response = client.update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert_eq!(errors[0]["registrationId"], json!(reg_id), "the conflict is keyed by registrationId: {body}");
    assert_eq!(errors[0]["error"]["status"], json!(409), "the unsupported operation surfaces a 409: {body}");
    assert_eq!(mock.get_count(Method::PATCH, &attrs_path).await, 0, "no forward on an unsupported operation");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(
        json_body(&local)["brandName"]["value"].as_str(),
        Some("BMW"),
        "the remainder is patched locally"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive update forwards the full remainder and also patches locally
/// (clause 5.5.8). Confirms REQ-PROV-18, 19.
#[tokio::test]
async fn update_attrs_inclusive_forwards_remainder_and_patches_local() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-update-inclusive");
    let reg_id = topology.id("registration:prov-update-inclusive");
    let attrs_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");

    let base = entity(
        &entity_id,
        "Vehicle",
        &[("brandName", property(json!("Mercedes"))), ("speed", property(json!(1)))],
    );
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&base)
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &attrs_path, 204).await;

    let fragment = attribute_fragment(&[("speed", property(json!(99))), ("brandName", property(json!("BMW")))]);
    let response = client.update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert!(
        response.status.as_u16() != 404,
        "the update is accepted: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded update body is JSON");
    assert!(
        attr_present(&forwarded_body, "speed"),
        "inclusive forwards the full remainder: {forwarded_body}"
    );
    assert!(
        attr_present(&forwarded_body, "brandName"),
        "inclusive forwards the full remainder: {forwarded_body}"
    );
    assert_eq!(mock.get_count(Method::PATCH, &attrs_path).await, 1, "the update forwards once");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["speed"]["value"].as_i64(), Some(99), "inclusive patches locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// End-to-end against real broker B: the owned `speed` update lands on B and is
/// never stored on A. Confirms REQ-PROV-14, 15, 17.
#[tokio::test]
async fn update_attrs_pair_exclusive_end_to_end() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let front = topology.front();
    let peer = topology.broker(BrokerRole::B);
    let entity_id = topology.id("Vehicle:prov-update-pair");
    let reg_id = topology.id("registration:prov-update-pair");

    let peer_entity = entity(&entity_id, "Vehicle", &[("speed", property(json!(1)))]);
    let created = peer.client().post_entity(&peer_entity).with_ld_json().send().await.expect("peer entity create");
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
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["updateAttrs", "retrieveEntity", "queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let fragment = attribute_fragment(&[("speed", property(json!(99)))]);
    let response = front.client().update_entity_attrs(&entity_id, &fragment).await.expect("update attributes");
    assert!(
        response.status.as_u16() != 404,
        "the update is accepted: {}",
        String::from_utf8_lossy(&response.body)
    );

    let peer_local = peer
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("peer local read");
    assert_status(&peer_local, 200);
    assert_eq!(
        json_body(&peer_local)["speed"]["value"].as_i64(),
        Some(99),
        "the owned update lands on broker B"
    );

    let front_local = front
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("front local read");
    assert_status(&front_local, 200);
    assert!(json_body(&front_local).get("speed").is_none(), "the owned attribute is not stored on broker A");

    topology.finish().await.expect("topology cleanup");
}
