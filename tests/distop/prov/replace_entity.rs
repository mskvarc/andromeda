//! Replace Entity forwarding — REQ-PROV-113..118 (clause 5.6.7.4).
//!
//! `PUT /entities/{id}` follows the same split as create: an exclusive
//! registration forwards its owned `speed` and strips it from the locally stored
//! replacement; an unadvertised `replaceEntity` yields a bare `409 Conflict`; an
//! inclusive registration forwards a full copy, replaces the entity locally, and
//! preserves its `createdAt` across the replacement (clause 5.5.6).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{CONFLICT, RESOURCE_NOT_FOUND};
use axum::http::Method;
use serde_json::{Value, json};

/// A `Vehicle` carrying the unowned `brandName` and the registration-owned `speed`.
fn brand_and_speed(id: &str, brand: &str, speed: i64) -> Value {
    entity(id, "Vehicle", &[("brandName", property(json!(brand))), ("speed", property(json!(speed)))])
}

/// Replacing an absent entity with no matching registration is a plain
/// `404 ResourceNotFound`. Confirms REQ-PROV-113 (positive).
#[tokio::test]
async fn replace_entity_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let entity_id = topology.id("Vehicle:prov-replace-absent");

    let replaced = client
        .replace_entity(&entity_id, &brand_and_speed(&entity_id, "Mercedes", 42), "application/ld+json", None)
        .await
        .expect("replace entity");
    assert_status(&replaced, 404);
    assert_problem_details(&replaced, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching registration for an entity absent locally forwards the replacement
/// rather than returning `404`. Confirms REQ-PROV-113 (negative).
#[tokio::test]
async fn replace_entity_registration_matches_entity_absent_not_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replace-absent-reg");
    let reg_id = topology.id("registration:prov-replace-absent-reg");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["replaceEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = client
        .replace_entity(&entity_id, &brand_and_speed(&entity_id, "Mercedes", 42), "application/ld+json", None)
        .await
        .expect("replace entity");
    assert!(
        replaced.status.as_u16() != 404,
        "a matching registration forwards rather than 404: {}",
        String::from_utf8_lossy(&replaced.body)
    );
    assert_eq!(mock.get_count(Method::PUT, &path), 1, "the replacement is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration forwards the owned `speed` and stores a local
/// replacement without it, keeping the unowned `brandName`. Confirms
/// REQ-PROV-114, 116.
#[tokio::test]
async fn replace_entity_exclusive_match_forwards_and_strips() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replace-exclusive");
    let reg_id = topology.id("registration:prov-replace-exclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["replaceEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_and_speed(&entity_id, "Mercedes", 42))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = client
        .replace_entity(&entity_id, &brand_and_speed(&entity_id, "BMW", 99), "application/ld+json", None)
        .await
        .expect("replace entity");
    assert!(
        replaced.status.as_u16() != 404,
        "the replacement is accepted: {}",
        String::from_utf8_lossy(&replaced.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded replace body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the owned attribute is forwarded: {forwarded_body}");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert!(local_body.get("brandName").is_some(), "the unowned attribute stays local: {local_body}");
    assert!(local_body.get("speed").is_none(), "the exclusive attribute is stripped locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration that does not advertise `replaceEntity` rejects the
/// wholly owned replacement with a bare `409 Conflict` and no forwarding.
/// Confirms REQ-PROV-115.
#[tokio::test]
async fn replace_entity_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replace-unsup");
    let reg_id = topology.id("registration:prov-replace-unsup");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&entity_id, "Vehicle", &[("speed", property(json!(42)))]))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let replaced = client
        .replace_entity(
            &entity_id,
            &entity(&entity_id, "Vehicle", &[("speed", property(json!(99)))]),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace entity");
    assert_status(&replaced, 409);
    assert_problem_details(&replaced, CONFLICT);
    assert_eq!(mock.get_count(Method::PUT, &path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards a full copy, fully replaces the local
/// entity, and preserves its `createdAt`. Confirms REQ-PROV-117, 118.
#[tokio::test]
async fn replace_entity_inclusive_forwards_remainder_replaces_local_preserves_createdat() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replace-inclusive");
    let reg_id = topology.id("registration:prov-replace-inclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_and_speed(&entity_id, "Mercedes", 42))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let before = client
        .get_entity(&entity_id, "local=true&options=sysAttrs", "application/ld+json", None)
        .await
        .expect("local read-back with system attributes");
    assert_status(&before, 200);
    let created_at = json_body(&before).get("createdAt").cloned();

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["replaceEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = client
        .replace_entity(&entity_id, &brand_and_speed(&entity_id, "BMW", 99), "application/ld+json", None)
        .await
        .expect("replace entity");
    assert!(
        replaced.status.as_u16() != 404,
        "the replacement is accepted: {}",
        String::from_utf8_lossy(&replaced.body)
    );
    assert_eq!(mock.get_count(Method::PUT, &path), 1, "inclusive forwards the replacement once");

    let after = client
        .get_entity(&entity_id, "local=true&options=sysAttrs", "application/ld+json", None)
        .await
        .expect("local read-back with system attributes");
    assert_status(&after, 200);
    let after_body = json_body(&after);
    assert_eq!(
        after_body["brandName"]["value"].as_str(),
        Some("BMW"),
        "the entity is fully replaced locally: {after_body}"
    );
    if let Some(created_at) = created_at.filter(|value| !value.is_null()) {
        assert_eq!(after_body.get("createdAt"), Some(&created_at), "createdAt is preserved across the replacement");
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
