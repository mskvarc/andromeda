//! Delete Attribute forwarding — REQ-PROV-34..40 (clause 5.6.5.4).
//!
//! An exclusive registration that owns the attribute forwards the `DELETE` and
//! terminates without removing the local copy; an unadvertised `deleteAttrs`
//! yields a bare `409 Conflict`, or the `409` keyed by `registrationId` inside
//! `207 Multi-Status` when another registration also participates; an inclusive
//! registration forwards the input and removes the attribute locally; and a
//! matching registration keeps a missing entity from being a `404` while a
//! locally present entity without the attribute still is one.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, request_path},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::{entity_attribute::DatasetSelection, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::problem_detail::{CONFLICT, RESOURCE_NOT_FOUND};
use axum::http::Method;
use serde_json::{Value, json};

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

/// Delete `speed` across all of its dataset instances.
async fn delete_speed(client: &TestClient, id: &str) -> TestResponse {
    client
        .delete_entity_attr(id, "speed", None, DatasetSelection::All, None)
        .await
        .expect("delete attribute")
}

/// An exclusive registration forwards the owned attribute delete and terminates:
/// the local copy is left in place. The registration precedes the local seed
/// because clause 5.9.2.4 rejects an exclusive registration when "an Entity
/// already exists for the supplied Entity ID (URI) and the existing Entity
/// contains any of the Attributes defined in the registration". Confirms
/// REQ-PROV-34, 35, 37.
#[tokio::test]
async fn delete_attr_exclusive_match_forwards_and_terminates() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delattr-exclusive");
    let reg_id = topology.id("registration:prov-delattr-exclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["deleteAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &entity_id, &[("speed", property(json!(42)))]).await;
    mock.set_stub(Method::DELETE, &path, 204).await;

    let response = delete_speed(client, &entity_id).await;
    assert_status(&response, 204);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with("/attrs/speed"),
        "the delete targets the owned attribute: {}",
        request_path(&forwarded)
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("speed").is_some(), "the local attribute is untouched after termination");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration that does not advertise `deleteAttrs` rejects the
/// delete it wholly owns with a bare `409 Conflict` and no forwarding. Confirms
/// REQ-PROV-36 (complete-fail).
#[tokio::test]
async fn delete_attr_exclusive_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delattr-unsup");
    let reg_id = topology.id("registration:prov-delattr-unsup");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &entity_id, &[("speed", property(json!(42)))]).await;

    let response = delete_speed(client, &entity_id).await;
    assert_status(&response, 409);
    assert_problem_details(&response, CONFLICT);
    assert_eq!(mock.get_count(Method::DELETE, &path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With two matching registrations — one advertising the operation, one not — the
/// delete is `207 Multi-Status`: the supporting source is forwarded to, the other
/// yields a `409` keyed by its `registrationId`. Both are redirect registrations
/// (clause 4.3.6.3: "In the case that multiple overlapping redirect
/// registrations are defined, operations are distributed to all registered
/// Context Sources"; a second exclusive on the same entity and attribute is a
/// clause 5.9.2.4 Conflict), registered before the local copy is seeded.
/// Confirms REQ-PROV-36 (partial), 39.
#[tokio::test]
async fn delete_attr_op_unsupported_multi_returns_207() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delattr-multi");
    let reg_supported = topology.id("registration:prov-delattr-multi-ok");
    let reg_unsupported = topology.id("registration:prov-delattr-multi-no");
    let path_supported = format!("/s1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    let path_unsupported = format!("/s2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let supported = CsrBuilder::new(&reg_supported, &mock.url("/s1"))
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["deleteAttrs", "retrieveEntity"])
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
    mock.set_stub(Method::DELETE, &path_supported, 204).await;

    let response = delete_speed(client, &entity_id).await;
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert!(
        errors
            .iter()
            .any(|entry| entry["registrationId"] == json!(reg_unsupported) && entry["error"]["status"] == json!(409)),
        "the unsupported registration surfaces a keyed 409: {body}"
    );
    assert_eq!(mock.get_count(Method::DELETE, &path_supported), 1, "the supporting source is forwarded to");
    assert_eq!(
        mock.get_count(Method::DELETE, &path_unsupported),
        0,
        "the unsupported source is not forwarded to"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With neither a local entity nor a matching registration, a delete attribute is
/// `404 ResourceNotFound`. Confirms REQ-PROV-38 (positive).
#[tokio::test]
async fn delete_attr_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-delattr-absent");

    let response = delete_speed(client, &entity_id).await;
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching registration keeps a delete on a locally absent entity from being a
/// `404`; the operation is forwarded. Confirms REQ-PROV-38 (negative).
#[tokio::test]
async fn delete_attr_registration_matches_entity_absent_not_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delattr-reg-absent");
    let reg_id = topology.id("registration:prov-delattr-reg-absent");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["deleteAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::DELETE, &path, 204).await;

    let response = delete_speed(client, &entity_id).await;
    assert!(
        response.status.as_u16() != 404,
        "a matched registration must not 404: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::DELETE, &path), 1, "the delete is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards the input and removes the attribute
/// locally. Confirms REQ-PROV-39.
#[tokio::test]
async fn delete_attr_inclusive_forwards_input() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delattr-inclusive");
    let reg_id = topology.id("registration:prov-delattr-inclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    seed_local(
        client,
        &entity_id,
        &[("brandName", property(json!("Mercedes"))), ("speed", property(json!(42)))],
    )
    .await;
    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["deleteAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::DELETE, &path, 204).await;

    let response = delete_speed(client, &entity_id).await;
    assert_status(&response, 204);
    assert_eq!(mock.get_count(Method::DELETE, &path), 1, "the delete is forwarded once");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert!(json_body(&local).get("speed").is_none(), "the attribute is removed locally");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A locally present entity that lacks the targeted attribute is `404` for a
/// delete attribute. Confirms REQ-PROV-40.
#[tokio::test]
async fn delete_attr_local_present_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-delattr-attr-absent");

    seed_local(client, &entity_id, &[("brandName", property(json!("Mercedes")))]).await;

    let response = delete_speed(client, &entity_id).await;
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}
