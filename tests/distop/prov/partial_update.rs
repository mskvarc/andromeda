//! Partial Attribute Update forwarding — REQ-PROV-27..33 (clause 5.6.4.4).
//!
//! A partial update targets one attribute. An exclusive registration that owns
//! it forwards the `PATCH` and terminates without touching the local copy; an
//! unadvertised operation yields a bare `409 Conflict`, or the `409` keyed by
//! `registrationId` inside `207 Multi-Status` when another registration also
//! participates; an inclusive registration forwards the input and patches
//! locally; and a matching registration keeps a missing entity from being a
//! `404` while a locally present entity without the attribute still is one.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, request_path},
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

/// An Attribute Fragment setting a new `value` for the targeted attribute, the
/// payload shape clause 5.6.4.3 defines for a partial update ("A JSON-LD
/// document representing an NGSI-LD Attribute Fragment"): the attribute's own
/// members, not an entity fragment keyed by attribute name.
fn speed_fragment(value: i64) -> Value {
    json!({ "value": value, "@context": COMPOUND_CONTEXT.as_str() })
}

/// A locally seeded `Vehicle` carrying the named attributes, created with
/// `local=true` so no registration forwarding runs during setup.
async fn seed_local(client: &TestClient, id: &str, attributes: &[(&str, Value)]) {
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(id, "Vehicle", attributes))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);
}

/// With neither a local entity nor a matching registration, a partial update is
/// `404 ResourceNotFound`. Confirms REQ-PROV-27 (positive).
#[tokio::test]
async fn partial_update_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-partial-absent");

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching registration keeps a partial update on a locally absent entity from
/// being a `404`; the operation is forwarded. Confirms REQ-PROV-27 (negative).
#[tokio::test]
async fn partial_update_registration_matches_entity_absent_not_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-partial-reg-absent");
    let reg_id = topology.id("registration:prov-partial-reg-absent");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &path, 204).await;

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert!(
        response.status.as_u16() != 404,
        "a matched registration must not 404: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::PATCH, &path), 1, "the partial update is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration forwards the owned attribute and terminates: the
/// local value is left unchanged. The forward names the attribute in the path
/// and carries the Attribute Fragment as its body (clauses 5.6.4.3, 6.6.3.1).
/// Confirms REQ-PROV-28, 29, 31.
#[tokio::test]
async fn partial_update_exclusive_match_forwards_and_terminates() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-partial-exclusive");
    let reg_id = topology.id("registration:prov-partial-exclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &entity_id, &[("speed", property(json!(42)))]).await;
    mock.set_stub(Method::PATCH, &path, 204).await;

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert_status(&response, 204);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with("/attrs/speed"),
        "the owned attribute is named by the forwarded path: {}",
        request_path(&forwarded)
    );
    let forwarded_body = forwarded.json::<Value>().expect("forwarded patch body is JSON");
    assert_eq!(
        forwarded_body["value"].as_i64(),
        Some(99),
        "the forwarded body is the Attribute Fragment itself: {forwarded_body}"
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(
        json_body(&local)["speed"]["value"].as_i64(),
        Some(42),
        "the local value is untouched after termination"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration that does not advertise `updateAttrs` rejects the
/// partial update it wholly owns with a bare `409 Conflict` and no forwarding.
/// Confirms REQ-PROV-30 (complete-fail).
#[tokio::test]
async fn partial_update_exclusive_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-partial-unsup");
    let reg_id = topology.id("registration:prov-partial-unsup");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &entity_id, &[("speed", property(json!(42)))]).await;

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert_status(&response, 409);
    assert_problem_details(&response, CONFLICT);
    assert_eq!(mock.get_count(Method::PATCH, &path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With two matching registrations — one advertising the operation, one not — the
/// partial update is `207 Multi-Status`: the supporting source is forwarded to,
/// the other yields a `409` keyed by its `registrationId`. Both are redirect
/// registrations: clause 4.3.6.3 allows overlapping redirects ("In the case
/// that multiple overlapping redirect registrations are defined, operations are
/// distributed to all registered Context Sources") while a second exclusive on
/// the same entity and attribute is a clause 5.9.2.4 Conflict. They are
/// registered before the local copy is seeded, since clause 5.9.2.4 rejects a
/// redirect when "an existing Entity already matches the Context Source
/// Registration". Confirms REQ-PROV-30 (partial), 32.
#[tokio::test]
async fn partial_update_op_unsupported_multi_returns_207() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-partial-multi");
    let reg_supported = topology.id("registration:prov-partial-multi-ok");
    let reg_unsupported = topology.id("registration:prov-partial-multi-no");
    let path_supported = format!("/s1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    let path_unsupported = format!("/s2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let supported = CsrBuilder::new(&reg_supported, &mock.url("/s1"))
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&supported).await.expect("create supported registration"), 201);
    let unsupported = CsrBuilder::new(&reg_unsupported, &mock.url("/s2"))
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&unsupported).await.expect("create unsupported registration"), 201);
    seed_local(client, &entity_id, &[("speed", property(json!(42)))]).await;
    mock.set_stub(Method::PATCH, &path_supported, 204).await;

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert!(
        errors
            .iter()
            .any(|entry| entry["registrationId"] == json!(reg_unsupported) && entry["error"]["status"] == json!(409)),
        "the unsupported registration surfaces a keyed 409: {body}"
    );
    assert_eq!(mock.get_count(Method::PATCH, &path_supported), 1, "the supporting source is forwarded to");
    assert_eq!(
        mock.get_count(Method::PATCH, &path_unsupported),
        0,
        "the unsupported source is not forwarded to"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards the input and also patches the local copy.
/// Confirms REQ-PROV-32.
#[tokio::test]
async fn partial_update_inclusive_forwards_input() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-partial-inclusive");
    let reg_id = topology.id("registration:prov-partial-inclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    seed_local(client, &entity_id, &[("speed", property(json!(42)))]).await;
    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &path, 204).await;

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert_status(&response, 204);
    assert_eq!(mock.get_count(Method::PATCH, &path), 1, "the partial update is forwarded once");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["speed"]["value"].as_i64(), Some(99), "the local copy is patched");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A locally present entity that lacks the targeted attribute is `404` for a
/// partial update. Confirms REQ-PROV-33.
#[tokio::test]
async fn partial_update_local_present_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-partial-attr-absent");

    seed_local(client, &entity_id, &[("brandName", property(json!("Mercedes")))]).await;

    let response = client
        .partial_update_attr(&entity_id, "speed", &speed_fragment(99))
        .await
        .expect("partial update");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}
