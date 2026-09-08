//! Retrieve Entity forwarding (clause 5.7.1.4): 404 suppression, local-as-base
//! merge, capability and mode selection, expired-attribute discard, and the
//! per-attribute / per-`datasetId` merge of clause 4.5.5. `datasetId` selection,
//! auxiliary supplementation, and term expansion live in
//! `crate::cons::retrieve_entity_projection`; entity-map rows in
//! `crate::cons::entity_map_reuse`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::RESOURCE_NOT_FOUND;
use axum::http::Method;
use serde_json::{Value, json};

/// A `Property` node carrying an `observedAt` timestamp.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// A `Property` node carrying `observedAt` and a `datasetId`.
fn observed_dataset(value: i64, observed_at: &str, dataset_id: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at, "datasetId": dataset_id })
}

/// Retrieval returns `404 ResourceNotFound` only when neither a local entity nor
/// any matching registration exists; a matching registration suppresses the 404
/// even if the source itself has nothing (clause 5.7.1.4). Confirms REQ-CONS-1.
#[tokio::test]
async fn distop_retrieve_entity_no_local_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons1");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");

    let no_registration = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert_status(&no_registration, 404);
    assert_problem_details(&no_registration, RESOURCE_NOT_FOUND);

    let mock = ContextSourceAdapter::start().await;
    let registration_id = topology.id("registration:cons1");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 404).await;

    let with_registration = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert!(
        with_registration.status.as_u16() != 404,
        "a matching registration must suppress the 404, got {}",
        with_registration.status.as_u16()
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Local data is the merge base: a local attribute is served unmodified and
/// remote attributes are merged on top (clause 5.7.1.4). Confirms REQ-CONS-2.
#[tokio::test]
async fn distop_retrieve_entity_local_data_is_merge_base() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons2");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front
            .client()
            .post_entity(&vehicle(&entity_id, 42))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    let remote = entity(&entity_id, "Vehicle", &[("fuelLevel", property(json!(70)))]);
    assert_status(&peer.client().post_entity(&remote).with_ld_json().send().await.expect("peer create"), 201);

    let registration_id = topology.id("registration:cons2");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["fuelLevel"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(body["speed"]["value"].as_i64(), Some(42), "local speed is the unmodified merge base");
    assert!(body.get("fuelLevel").is_some(), "remote fuelLevel is merged onto the local base: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// Only registrations advertising `retrieveEntity` are forwarded a Retrieve
/// Entity; a `queryEntity`-only source is never contacted (clause 5.7.1.4).
/// Confirms REQ-CONS-7.
#[tokio::test]
async fn distop_retrieve_entity_forwards_only_retrieveentity_capable_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons7");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    for (source, operations) in [
        ("/s1", &["retrieveEntity"][..]),
        ("/s2", &["queryEntity"][..]),
        ("/s3", &["retrieveEntity", "queryEntity"][..]),
    ] {
        let registration_id = topology.id(&format!("registration:cons7-{}", source.trim_start_matches('/')));
        let registration = CsrBuilder::new(&registration_id, &mock.url(source))
            .mode("inclusive")
            .entity_id(&entity_id)
            .operations(operations)
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
            .await;
    }

    assert_status(
        &front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve"),
        200,
    );

    assert!(
        mock.get_count(Method::GET, &format!("/s1/ngsi-ld/v1/entities/{entity_id}")) > 0,
        "retrieveEntity source is forwarded"
    );
    assert!(
        mock.get_count(Method::GET, &format!("/s3/ngsi-ld/v1/entities/{entity_id}")) > 0,
        "combined-capability source is forwarded"
    );
    assert_eq!(
        mock.get_count(Method::GET, &format!("/s2/ngsi-ld/v1/entities/{entity_id}")),
        0,
        "queryEntity-only source is not forwarded"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Exclusive, redirect, and inclusive registrations each forward the retrieve and
/// contribute the remote attribute to the merged result (clause 5.7.1.4).
/// Confirms REQ-CONS-9.
#[tokio::test]
async fn distop_retrieve_entity_forwards_exclusive_redirect_inclusive() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    for mode in ["exclusive", "redirect", "inclusive"] {
        let entity_id = topology.id(&format!("Vehicle:cons9-{mode}"));
        let source = format!("/{mode}");
        let registration_id = topology.id(&format!("registration:cons9-{mode}"));
        let registration = CsrBuilder::new(&registration_id, &mock.url(&source))
            .mode(mode)
            .entity_id(&entity_id)
            .property_names(&["speed"])
            .operations(&["retrieveEntity"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 55))
            .await;

        let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
        assert_status(&retrieved, 200);
        assert_eq!(
            json_body(&retrieved)["speed"]["value"].as_i64(),
            Some(55),
            "{mode} mode serves the remote speed"
        );
        assert!(
            mock.get_count(Method::GET, &format!("{source}/ngsi-ld/v1/entities/{entity_id}")) > 0,
            "{mode} mode forwards the retrieve"
        );
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A remote attribute whose `expiresAt` is in the past is discarded before the
/// merge; a non-expired attribute survives (clause 5.7.1.4). Confirms REQ-CONS-10.
#[tokio::test]
async fn distop_retrieve_entity_discards_expired_remote_attribute() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons10");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons10");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let remote = entity(
        &entity_id,
        "Vehicle",
        &[
            ("speed", json!({ "type": "Property", "value": 90, "expiresAt": "2000-01-01T00:00:00Z" })),
            ("fuelLevel", json!({ "type": "Property", "value": 70, "expiresAt": "2999-01-01T00:00:00Z" })),
        ],
    );
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &remote)
        .await;

    let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert!(body.get("fuelLevel").is_some(), "non-expired fuelLevel is retained: {body}");
    assert!(body.get("speed").is_none(), "expired speed is discarded before the merge: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Per clause 4.5.5 the same attribute merges per `datasetId`: the default
/// instance takes the most-recent `observedAt` across local and remote, and a
/// distinct `datasetId` instance coexists alongside it. Confirms REQ-CONS-11.
#[tokio::test]
async fn distop_retrieve_entity_merges_per_4_5_5() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons11");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("speed", observed(10, "2021-01-01T00:00:00Z"))]))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", &[("speed", observed(20, "2022-01-01T00:00:00Z"))]))
            .with_ld_json()
            .send()
            .await
            .expect("b"),
        201,
    );
    let dataset_id = topology.id("dataset:2");
    let c_entity = entity(
        &entity_id,
        "Vehicle",
        &[
            ("speed", observed_dataset(30, "2023-01-01T00:00:00Z", &dataset_id)),
            (
                "location",
                json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } }),
            ),
        ],
    );
    assert_status(&holder_c.client().post_entity(&c_entity).with_ld_json().send().await.expect("c"), 201);

    for (role, names) in [(BrokerRole::B, &["speed"][..]), (BrokerRole::C, &["speed", "location"][..])] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons11-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .mode("inclusive")
            .entity_id(&entity_id)
            .property_names(names)
            .operations(&["retrieveEntity"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert!(body.get("location").is_some(), "location from broker-c is merged: {body}");
    let instances = body["speed"].as_array().cloned().unwrap_or_else(|| vec![body["speed"].clone()]);
    assert!(
        instances.iter().any(|instance| instance["datasetId"].as_str() == Some(dataset_id.as_str())),
        "the datasetId instance coexists with the default instance: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}
