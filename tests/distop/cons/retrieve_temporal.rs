//! Retrieve Temporal Entity forwarding (clause 5.7.3.4): 404 suppression, local
//! evolution as the base series, pre-distribution temporal filtering, capability
//! selection, and the 4.5.5 merge of remote temporal instances into S1.
//! Auxiliary, `datasetId`, and `lastN` rows live in
//! `crate::cons::retrieve_temporal_filters`; map rows in
//! `crate::cons::entity_map_temporal`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::RESOURCE_NOT_FOUND;
use axum::http::Method;
use serde_json::{Value, json};

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// The whole-history temporal query window: everything before 2030.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// Retrieve Temporal returns `404 ResourceNotFound` only when neither a local
/// evolution nor a matching registration exists (clause 5.7.3.4). Confirms
/// REQ-CONS-39.
#[tokio::test]
async fn distop_retrieve_temporal_no_local_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons39");
    let front = topology.broker(BrokerRole::A);

    let no_registration = front.client().get_temporal_entity(&entity_id, BEFORE_2030, None).await.expect("retrieve");
    assert_status(&no_registration, 404);
    assert_problem_details(&no_registration, RESOURCE_NOT_FOUND);

    let mock = ContextSourceAdapter::start().await;
    let registration_id = topology.id("registration:cons39");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"),
        200,
        &entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]),
    )
    .await;

    let with_registration = front.client().get_temporal_entity(&entity_id, BEFORE_2030, None).await.expect("retrieve");
    assert!(
        with_registration.status.as_u16() != 404,
        "a matching registration suppresses the 404, got {}",
        with_registration.status.as_u16()
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local temporal evolution is the base series onto which a remote attribute
/// evolution is merged (clause 5.7.3.4). Confirms REQ-CONS-40.
#[tokio::test]
async fn distop_retrieve_temporal_local_evolution_is_base_s() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons40");
    let front = topology.broker(BrokerRole::A);
    let peer = topology.broker(BrokerRole::B);

    let local = entity(
        &entity_id,
        "Vehicle",
        &[(
            "speed",
            json!([
                observed(120, "2020-08-01T12:00:00Z"),
                observed(80, "2020-08-01T12:05:00Z"),
                observed(100, "2020-08-01T12:10:00Z")
            ]),
        )],
    );
    assert_status(&front.client().create_temporal_entity(&local).await.expect("local"), 201);
    let remote = entity(
        &entity_id,
        "Vehicle",
        &[("fuelLevel", json!([observed(70, "2020-08-01T12:00:00Z"), observed(50, "2020-08-01T12:10:00Z")]))],
    );
    assert_status(&peer.client().create_temporal_entity(&remote).await.expect("peer"), 201);

    let registration_id = topology.id("registration:cons40");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["fuelLevel"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front.client().get_temporal_entity(&entity_id, BEFORE_2030, None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert!(body.get("speed").is_some(), "the local speed history is the base series: {body}");
    assert!(body.get("fuelLevel").is_some(), "the remote fuelLevel history is merged: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// Local temporal filtering restricts the base series to the requested window
/// before distribution: a `between` query excludes an out-of-window instance
/// (clause 5.7.3.4). Confirms REQ-CONS-41.
#[tokio::test]
async fn distop_retrieve_temporal_local_temporal_filtering_produces_s1() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons41");
    let front = topology.broker(BrokerRole::A);

    let local = entity(
        &entity_id,
        "Vehicle",
        &[(
            "speed",
            json!([
                observed(120, "2020-08-01T12:00:00Z"),
                observed(80, "2020-08-01T12:05:00Z"),
                observed(100, "2020-08-01T12:10:00Z")
            ]),
        )],
    );
    assert_status(&front.client().create_temporal_entity(&local).await.expect("local"), 201);

    let windowed = front
        .client()
        .get_temporal_entity(&entity_id, "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T12:06:00Z", None)
        .await
        .expect("retrieve");
    assert_status(&windowed, 200);
    let instances = json_body(&windowed)["speed"].as_array().cloned().unwrap_or_default();
    assert!(!instances.is_empty(), "in-window instances are retained");
    assert!(
        instances
            .iter()
            .all(|instance| instance["observedAt"].as_str().is_none_or(|at| at <= "2020-08-01T12:06:00Z")),
        "the out-of-window T2 instance is excluded before distribution: {instances:?}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Retrieve Temporal forwards only to `retrieveTemporal`-capable sources; a
/// `queryTemporal`-only source is not contacted (clause 5.7.3.4). Confirms
/// REQ-CONS-44.
#[tokio::test]
async fn distop_retrieve_temporal_forwards_only_retrievetemporal_capable() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons44");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    for (source, operations) in [("/s1", &["retrieveTemporal"][..]), ("/s2", &["queryTemporal"][..])] {
        let registration_id = topology.id(&format!("registration:cons44-{}", source.trim_start_matches('/')));
        let registration = CsrBuilder::new(&registration_id, &mock.url(source))
            .mode("inclusive")
            .entity_id(&entity_id)
            .operations(operations)
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(
            Method::GET,
            &format!("{source}/ngsi-ld/v1/temporal/entities/{entity_id}"),
            200,
            &entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]),
        )
        .await;
    }

    assert_status(&front.client().get_temporal_entity(&entity_id, BEFORE_2030, None).await.expect("retrieve"), 200);
    assert!(
        mock.get_count(Method::GET, &format!("/s1/ngsi-ld/v1/temporal/entities/{entity_id}")).await > 0,
        "retrieveTemporal source is forwarded"
    );
    assert_eq!(
        mock.get_count(Method::GET, &format!("/s2/ngsi-ld/v1/temporal/entities/{entity_id}")).await,
        0,
        "queryTemporal-only source is not forwarded"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Remote temporal instances from split holders merge into S1 per clause 4.5.5,
/// so the result carries both attribute histories (clause 5.7.3.4). Confirms
/// REQ-CONS-46.
#[tokio::test]
async fn distop_retrieve_temporal_merges_modes_into_s1() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons46");
    let front = topology.broker(BrokerRole::A);
    let holder_b = topology.broker(BrokerRole::B);
    let holder_c = topology.broker(BrokerRole::C);

    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(120, "2020-08-01T12:00:00Z"), observed(80, "2020-08-01T12:05:00Z")]))],
            ))
            .await
            .expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[(
                    "location",
                    json!([{ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] }, "observedAt": "2020-08-01T12:00:00Z" }]),
                )],
            ))
            .await
            .expect("c"),
        201,
    );

    for (role, names) in [(BrokerRole::B, &["speed"][..]), (BrokerRole::C, &["location"][..])] {
        let peer = topology.broker(role);
        let registration_id = topology.id(&format!("registration:cons46-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .mode("inclusive")
            .entity_id(&entity_id)
            .property_names(names)
            .operations(&["retrieveTemporal"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let retrieved = front.client().get_temporal_entity(&entity_id, BEFORE_2030, None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert!(body.get("speed").is_some(), "the speed history from broker-b is merged: {body}");
    assert!(body.get("location").is_some(), "the location history from broker-c is merged: {body}");

    topology.finish().await.expect("topology cleanup");
}
