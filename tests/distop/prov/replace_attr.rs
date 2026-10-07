//! Replace Attribute forwarding (clause 5.6.19.4).
//!
//! `PUT /entities/{id}/attrs/{attr}` replaces a single attribute. An exclusive
//! registration that owns the attribute forwards the replacement and terminates
//! without touching the local copy; an unadvertised `replaceAttrs` (the Table
//! 4.20-1 name of 5.6.19 Replace Attribute) yields a bare `409 Conflict`; an
//! inclusive registration forwards a copy and replaces the attribute locally.
//! Exclusive rows register before seeding the local copy, since clause 5.9.2.4
//! rejects an exclusive registration over an attribute the entity already holds.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_problem_details, assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, request_path},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, CONFLICT, RESOURCE_NOT_FOUND},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A single `speed` Property node carrying the compound context, for `PUT` of one
/// attribute.
fn speed_node(value: i64) -> Value {
    json!({ "type": "Property", "value": value, "@context": COMPOUND_CONTEXT.as_str() })
}

/// Issue the replace-attribute request against broker A for the `speed` attribute.
async fn replace_speed(topology: &MultiBrokerTopology<BrokerRole>, entity_id: &str, value: i64) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed"))
        .header("content-type", "application/ld+json")
        .json(&speed_node(value))
        .send()
        .await
        .expect("replace attr")
}

/// Replacing an attribute on an absent entity with no matching registration is a
/// plain `404 ResourceNotFound`.
#[tokio::test]
async fn replace_attr_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:prov-replaceattr-absent");

    let replaced = replace_speed(&topology, &entity_id, 99).await;
    assert_status(&replaced, 404);
    assert_problem_details(&replaced, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A matching registration for an entity absent locally forwards the attribute
/// replacement rather than returning `404`.
#[tokio::test]
async fn replace_attr_registration_matches_entity_absent_not_404_and_forwards() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replaceattr-absent-reg");
    let reg_id = topology.id("registration:prov-replaceattr-absent-reg");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["replaceAttrs", "retrieveEntity"])
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
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = replace_speed(&topology, &entity_id, 99).await;
    assert!(
        replaced.status.as_u16() != 404,
        "a matching registration forwards rather than 404: {}",
        String::from_utf8_lossy(&replaced.body)
    );
    assert_eq!(mock.get_count(Method::PUT, &path), 1, "the attribute replacement is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration that does not advertise `replaceAttrs` (the Table
/// 4.20-1 name of 5.6.19) rejects the owned attribute replacement with a bare
/// `409 Conflict` and no forwarding.
#[tokio::test]
async fn replace_attr_op_unsupported_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replaceattr-unsup");
    let reg_id = topology.id("registration:prov-replaceattr-unsup");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

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

    let replaced = replace_speed(&topology, &entity_id, 99).await;
    assert_status(&replaced, 409);
    assert_problem_details(&replaced, CONFLICT);
    assert_eq!(mock.get_count(Method::PUT, &path), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration forwards the owned attribute replacement and
/// terminates without applying it locally.
#[tokio::test]
async fn replace_attr_exclusive_match_terminates_after_forwarding() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replaceattr-terminate");
    let reg_id = topology.id("registration:prov-replaceattr-terminate");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["replaceAttrs", "retrieveEntity"])
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
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = replace_speed(&topology, &entity_id, 99).await;
    assert!(
        replaced.status.as_u16() != 404,
        "the replacement is accepted: {}",
        String::from_utf8_lossy(&replaced.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with("/attrs/speed"),
        "the owned attribute replacement is forwarded: {}",
        request_path(&forwarded)
    );

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(
        json_body(&local)["speed"]["value"].as_i64(),
        Some(42),
        "the local attribute is unchanged after forwarding"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards the attribute replacement and also replaces
/// it locally.
#[tokio::test]
async fn replace_attr_inclusive_forwards_remainder() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replaceattr-inclusive");
    let reg_id = topology.id("registration:prov-replaceattr-inclusive");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&entity_id, "Vehicle", &[("speed", property(json!(42)))]))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["replaceAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = replace_speed(&topology, &entity_id, 99).await;
    assert!(
        replaced.status.as_u16() != 404,
        "the replacement is accepted: {}",
        String::from_utf8_lossy(&replaced.body)
    );
    assert_eq!(mock.get_count(Method::PUT, &path), 1, "inclusive forwards the replacement once");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["speed"]["value"].as_i64(), Some(99), "the local attribute is replaced");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 5.6.19.4 runs the clause 5.5.4 validation of the replacement before
/// its forwarding phase, and the replacement is a whole instance (Table
/// 5.2.5-1), so a Fragment naming only a `unitCode` is a `400 BadRequestData`
/// that no inclusive registration receives, and the local `speed` is untouched.
#[tokio::test]
async fn replace_attr_without_a_value_is_refused_before_forwarding_to_an_inclusive_registration() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replaceattr-valueless");
    let reg_id = topology.id("registration:prov-replaceattr-valueless");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&entity_id, "Vehicle", &[("speed", property(json!(42)))]))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["replaceAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PUT, &path, 204).await;

    let replaced = client
        .raw(Method::PUT, &path)
        .header("content-type", "application/ld+json")
        .json(&json!({ "unitCode": "C62", "@context": COMPOUND_CONTEXT.as_str() }))
        .send()
        .await
        .expect("replace attr");
    assert_status(&replaced, 400);
    assert_problem_details(&replaced, BAD_REQUEST_DATA);
    assert_eq!(mock.get_count(Method::PUT, &path), 0, "a replacement clause 5.5.4 refuses is never forwarded");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["speed"]["value"].as_i64(), Some(42), "the local attribute is unchanged");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Issue the replace-attribute request against broker A for `speed`, naming the
/// target entity's type with `selector` (Table 6.7.3.3-1 `type`).
async fn replace_speed_selecting(topology: &MultiBrokerTopology<BrokerRole>, entity_id: &str, selector: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed?type={selector}"))
        .header("content-type", "application/ld+json")
        .json(&speed_node(99))
        .send()
        .await
        .expect("replace attr")
}

/// Clause 5.12 matches a write to a registration by the request's type
/// selector: a selector naming `Building` does not match an exclusive
/// registration of `Vehicle` entities that covers the entity identifier, so
/// nothing is forwarded and the entity, held nowhere else, is `404
/// ResourceNotFound`; naming `Vehicle` matches and forwards.
#[tokio::test]
async fn replace_attr_type_selector_decides_which_registration_receives_the_write() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-replaceattr-selector");
    let reg_id = topology.id("registration:prov-replaceattr-selector");
    let path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .entity_type("Vehicle")
        .property_names(&["speed"])
        .operations(&["replaceAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PUT, &path, 204).await;

    let excluded = replace_speed_selecting(&topology, &entity_id, "Building").await;
    assert_status(&excluded, 404);
    assert_problem_details(&excluded, RESOURCE_NOT_FOUND);
    assert_eq!(mock.get_count(Method::PUT, &path), 0, "a registration of another type is not sent the write");

    let matched = replace_speed_selecting(&topology, &entity_id, "Vehicle").await;
    assert!(
        matched.status.as_u16() != 404,
        "a registration of the selected type receives the write: {}",
        String::from_utf8_lossy(&matched.body)
    );
    assert_eq!(mock.get_count(Method::PUT, &path), 1, "the write is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Replacing an attribute absent from a locally present entity with no matching
/// registration is `404 ResourceNotFound`.
#[tokio::test]
async fn replace_attr_local_present_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let entity_id = topology.id("Vehicle:prov-replaceattr-missing");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&entity_id, "Vehicle", &[("brandName", property(json!("Mercedes")))]))
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);

    let replaced = replace_speed(&topology, &entity_id, 99).await;
    assert_status(&replaced, 404);

    topology.finish().await.expect("topology cleanup");
}
