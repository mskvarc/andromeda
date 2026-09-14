//! Create Entity forwarding — REQ-PROV-1..12 (clause 5.6.1.4).
//!
//! Exclusive/redirect registrations own `speed` and forward it while the local
//! fragment keeps the remainder (`brandName`); an unadvertised `createEntity`
//! yields `409 Conflict` bare or keyed inside `207 Multi-Status`; inclusive
//! forwards a full copy and also writes locally; and a `T-PAIR` row proves the
//! split end-to-end against real broker B.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_problem_details, assert_status, forwarded_header, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::CONFLICT;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A `Vehicle` carrying the unowned `brandName` and the registration-owned `speed`.
fn brand_and_speed(id: &str) -> Value {
    entity(id, "Vehicle", &[("brandName", property(json!("Mercedes"))), ("speed", property(json!(42)))])
}

/// Exclusive create forwards the owned `speed`, strips it from the local
/// fragment, and keeps the unowned `brandName` locally. Confirms REQ-PROV-1, 2, 5.
#[tokio::test]
async fn create_exclusive_match_forwards_speed_and_strips_from_fragment() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-exclusive");
    let reg_id = topology.id("registration:prov-create-exclusive");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 201);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded create body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "the exclusive attribute is forwarded: {forwarded_body}");
    assert!(
        !attr_present(&forwarded_body, "brandName"),
        "the unowned attribute is not forwarded: {forwarded_body}"
    );
    assert!(
        forwarded_header(&forwarded, "content-type").is_some_and(|value| value.contains("json")),
        "the forwarded create carries a JSON content type"
    );

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

/// A create endpoint error on an exclusive forward yields `207 Multi-Status`
/// keyed by `registrationId`, while the unowned remainder is created locally.
/// Confirms REQ-PROV-3.
#[tokio::test]
async fn create_exclusive_endpoint_error_returns_partial() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-excl-err");
    let reg_id = topology.id("registration:prov-create-excl-err");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 500).await;

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 207);
    let body = json_body(&created);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert_eq!(
        errors[0]["registrationId"],
        json!(reg_id),
        "the forwarding error is keyed by registrationId: {body}"
    );
    let status = errors[0]["error"]["status"].as_u64().expect("error carries a status");
    assert!((500..600).contains(&status), "the endpoint error maps to a 5xx status, got {status}");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("brandName").is_some(), "the unowned attribute is created locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration that does not advertise `createEntity` rejects a
/// create it wholly owns with a bare `409 Conflict` and no forwarding.
/// Confirms REQ-PROV-4 (complete-fail).
#[tokio::test]
async fn create_exclusive_createentity_unsupported_returns_409_conflict() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-excl-unsup");
    let reg_id = topology.id("registration:prov-create-excl-unsup");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let payload = entity(&entity_id, "Vehicle", &[("speed", property(json!(42)))]);
    let created = client.post_entity(&payload).with_ld_json().send().await.expect("create entity");
    assert_status(&created, 409);
    assert_problem_details(&created, CONFLICT);
    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entities"),
        0,
        "no forward on an unsupported operation"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The same unsupported `createEntity`, but with a local remainder, degrades to
/// `207 Multi-Status` carrying the `409` keyed by `registrationId`; the
/// remainder is created locally. Confirms REQ-PROV-4 (partial), 5, 12.
#[tokio::test]
async fn create_exclusive_createentity_unsupported_with_local_remainder_returns_207() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-excl-remainder");
    let reg_id = topology.id("registration:prov-create-excl-remainder");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 207);
    let body = json_body(&created);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert_eq!(errors[0]["registrationId"], json!(reg_id), "the conflict is keyed by registrationId: {body}");
    assert_eq!(errors[0]["error"]["status"], json!(409), "the unsupported operation surfaces a 409: {body}");
    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entities"),
        0,
        "no forward on an unsupported operation"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert!(local_body.get("brandName").is_some(), "the remainder is created locally: {local_body}");
    assert!(local_body.get("speed").is_none(), "the owned attribute is not stored locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration forwards the whole input and holds nothing locally.
/// Confirms REQ-PROV-6, 7, 9.
#[tokio::test]
async fn create_redirect_match_forwards_and_strips() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-redirect");
    let reg_id = topology.id("registration:prov-create-redirect");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 201);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded create body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "redirect forwards the whole input: {forwarded_body}");
    assert!(
        attr_present(&forwarded_body, "brandName"),
        "redirect forwards the whole input: {forwarded_body}"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect create endpoint error yields `207 Multi-Status` keyed by
/// `registrationId`, and the unowned remainder is created locally.
/// Confirms REQ-PROV-7.
#[tokio::test]
async fn create_redirect_endpoint_error_returns_partial() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-redir-err");
    let reg_id = topology.id("registration:prov-create-redir-err");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 502).await;

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 207);
    let body = json_body(&created);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert_eq!(
        errors[0]["registrationId"],
        json!(reg_id),
        "the forwarding error is keyed by registrationId: {body}"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("brandName").is_some(), "the unowned attribute is created locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration that does not advertise `createEntity` rejects the
/// create with a bare `409 Conflict` and no forwarding. Confirms REQ-PROV-8.
#[tokio::test]
async fn create_redirect_createentity_unsupported_returns_409_conflict() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-redir-unsup");
    let reg_id = topology.id("registration:prov-create-redir-unsup");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 409);
    assert_problem_details(&created, CONFLICT);
    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entities"),
        0,
        "no forward on an unsupported operation"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards a full copy and also creates the entity
/// locally. Confirms REQ-PROV-10, 12.
#[tokio::test]
async fn create_inclusive_forwards_remainder_and_creates_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-inclusive");
    let reg_id = topology.id("registration:prov-create-inclusive");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 201);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    let forwarded_body = forwarded.json::<Value>().expect("forwarded create body is JSON");
    assert!(attr_present(&forwarded_body, "speed"), "inclusive forwards the full input: {forwarded_body}");
    assert!(
        attr_present(&forwarded_body, "brandName"),
        "inclusive forwards the full input: {forwarded_body}"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert!(local_body.get("speed").is_some(), "inclusive keeps the full entity locally: {local_body}");
    assert!(local_body.get("brandName").is_some(), "inclusive keeps the full entity locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An entity already present locally under an inclusive registration surfaces a
/// keyed `409 AlreadyExists` in `207 Multi-Status`, while the forward still
/// occurs. Confirms REQ-PROV-11.
#[tokio::test]
async fn create_existing_locally_returns_already_exists() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-create-existing");
    let reg_id = topology.id("registration:prov-create-existing");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&brand_and_speed(&entity_id))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let created = client
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("recreate entity");
    assert_status(&created, 207);
    let body = json_body(&created);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert_eq!(errors[0]["error"]["status"], json!(409), "the local duplicate surfaces a 409: {body}");
    assert!(mock.get_count(Method::POST, "/ngsi-ld/v1/entities") >= 1, "the inclusive forward still occurs");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// End-to-end against real broker B: the owned `speed` lands on B, the remainder
/// stays on A, and a merged retrieve on A reunites them. Confirms REQ-PROV-1, 2,
/// 5, 12.
#[tokio::test]
async fn create_pair_exclusive_end_to_end() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let entity_id = topology.id("Vehicle:prov-create-pair");
    let reg_id = topology.id("registration:prov-create-pair");

    let registration = CsrBuilder::new(&reg_id, peer.peer_url())
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity", "retrieveEntity", "queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let created = front
        .client()
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("create entity");
    assert_status(&created, 201);

    let front_local = front
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("front local read");
    assert_status(&front_local, 200);
    let front_body = json_body(&front_local);
    assert!(front_body.get("brandName").is_some(), "the remainder stays on broker A: {front_body}");
    assert!(front_body.get("speed").is_none(), "the owned attribute is not on broker A: {front_body}");

    let peer_local = peer
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("peer local read");
    assert_status(&peer_local, 200);
    assert_eq!(
        json_body(&peer_local)["speed"]["value"].as_i64(),
        Some(42),
        "the owned attribute lands on broker B"
    );

    let merged = front
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("merged read");
    assert_status(&merged, 200);
    let merged_body = json_body(&merged);
    assert_eq!(merged_body["speed"]["value"].as_i64(), Some(42), "the merged view carries the owned attribute");
    assert!(merged_body.get("brandName").is_some(), "the merged view carries the remainder");

    topology.finish().await.expect("topology cleanup");
}
