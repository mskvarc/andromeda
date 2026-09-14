//! Query Temporal forwarding (clause 5.7.4.4/.5): capability selection, the 4.5.5
//! merge of per-mode temporal arrays into S4, auxiliary supplementation, and
//! post-aggregation `lastN`. Validation rows live in
//! `crate::cons::query_temporal_validation`; `datasetId`, pagination, csf, and
//! ordering in `crate::cons::query_temporal_filters`; map rows in
//! `crate::cons::entity_map_temporal`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// The temporal entity with the given id from a temporal array body.
fn temporal_entity(body: &Value, id: &str) -> Value {
    body.as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(id)))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The `observedAt` values of one attribute on a temporal entity.
fn observed_ats(entity: &Value, attribute: &str) -> Vec<String> {
    entity[attribute]
        .as_array()
        .map(|instances| {
            instances
                .iter()
                .filter_map(|instance| instance["observedAt"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// A non-local temporal query forwards only to `queryTemporal`-capable sources; a
/// `local=true` query forwards to none (clause 5.7.4.4). Confirms REQ-CONS-60.
#[tokio::test]
async fn distop_query_temporal_forwards_matching_querytemporal_local_none() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    for (source, operations) in [("/s1", &["queryTemporal"][..]), ("/s2", &["retrieveTemporal"][..])] {
        let registration_id = topology.id(&format!("registration:cons60-{}", source.trim_start_matches('/')));
        let registration = CsrBuilder::new(&registration_id, &mock.url(source))
            .mode("inclusive")
            .operations(operations)
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/temporal/entities"), 200, &json!([]))
            .await;
    }

    assert_status(
        &front
            .client()
            .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}"), None)
            .await
            .expect("query"),
        200,
    );
    assert!(
        mock.get_count(Method::GET, "/s1/ngsi-ld/v1/temporal/entities") > 0,
        "queryTemporal source is forwarded"
    );
    assert_eq!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/temporal/entities"),
        0,
        "retrieveTemporal-only source is not forwarded"
    );

    let total_before_local = mock.get_total_count();
    assert_status(
        &front
            .client()
            .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&local=true"), None)
            .await
            .expect("query"),
        200,
    );
    assert_eq!(mock.get_total_count(), total_before_local, "a local=true temporal query forwards to no source");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Per-mode temporal arrays merge into S4 per clause 4.5.5, so a same-id entity
/// carries the histories of every source and distinct-id entities coexist
/// (clause 5.7.4.4). Confirms REQ-CONS-63.
#[tokio::test]
async fn distop_query_temporal_merges_modes_into_s4() {
    let topology = topology::chain().await.expect("three-broker topology");
    let shared_id = topology.id("Vehicle:cons63-shared");
    let extra_id = topology.id("Vehicle:cons63-extra");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(&shared_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("b shared"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(
                &shared_id,
                "Vehicle",
                &[(
                    "location",
                    json!([{ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] }, "observedAt": "2020-08-01T12:00:00Z" }]),
                )],
            ))
            .await
            .expect("c shared"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(&extra_id, "Vehicle", &[("speed", json!([observed(40, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("c extra"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons63-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .mode("inclusive")
            .operations(&["queryTemporal"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    let merged = temporal_entity(&body, &shared_id);
    assert!(
        merged.get("speed").is_some() && merged.get("location").is_some(),
        "the same-id temporal entity merges both source histories: {merged}"
    );
    assert!(
        !temporal_entity(&body, &extra_id).is_null(),
        "the distinct-id temporal entity is present: {body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary source is merged into S4 only where absent: a duplicate
/// timeproperty instance is dropped while a new instance and a new attribute are
/// added (clause 5.7.4.4). Confirms REQ-CONS-64.
#[tokio::test]
#[ignore = "SPEC-CONFLICT: clause 5.9.2.4 limits auxiliary registrations to retrieveOps, retrieveEntity and queryEntity, so an auxiliary temporal source cannot be registered"]
async fn distop_query_temporal_auxiliary_merged_into_s4_only_if_absent() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons64");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("local"),
        201,
    );
    let auxiliary = entity(
        &entity_id,
        "Vehicle",
        &[
            ("speed", json!([observed(99, "2020-08-01T12:00:00Z"), observed(70, "2020-08-01T12:30:00Z")])),
            ("fuelLevel", json!([observed(40, "2020-08-01T12:30:00Z")])),
        ],
    );
    assert_status(&peer.client().create_temporal_entity(&auxiliary).await.expect("peer"), 201);

    let registration_id = topology.id("registration:cons64");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .mode("auxiliary")
        .operations(&["queryTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let merged = temporal_entity(&json_body(&response), &entity_id);
    let at_first: Vec<&Value> = merged["speed"]
        .as_array()
        .map(|instances| {
            instances
                .iter()
                .filter(|instance| instance["observedAt"].as_str() == Some("2020-08-01T12:00:00Z"))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(at_first.len(), 1, "the duplicate timeproperty instance is dropped: {merged}");
    assert_eq!(at_first[0]["value"].as_i64(), Some(50), "the present inclusive instance is kept");
    assert!(
        observed_ats(&merged, "speed").contains(&"2020-08-01T12:30:00Z".to_owned()),
        "the new auxiliary instance is added: {merged}"
    );
    assert!(merged.get("fuelLevel").is_some(), "the absent auxiliary attribute is added: {merged}");

    topology.finish().await.expect("topology cleanup");
}

/// `lastN` limits the projected temporal instances per entity to the most recent
/// across the aggregated sources (clause 5.7.4.4). Confirms REQ-CONS-69.
#[tokio::test]
async fn distop_query_temporal_lastn_limits_projected_post_aggregation() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons69");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder_b = topology.broker(BrokerRole::B).expect("topology broker B");
    let holder_c = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(10, "2020-08-01T12:00:00Z"), observed(20, "2020-08-01T12:01:00Z")]))],
            ))
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(30, "2020-08-01T12:02:00Z"), observed(40, "2020-08-01T12:03:00Z")]))],
            ))
            .await
            .expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:04:00Z")]))]))
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role).expect("topology broker for role");
        let registration_id = topology.id(&format!("registration:cons69-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .mode("inclusive")
            .operations(&["queryTemporal"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&attrs=speed&lastN=2"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let mut ats = observed_ats(&temporal_entity(&json_body(&response), &entity_id), "speed");
    ats.sort();
    assert_eq!(
        ats,
        vec!["2020-08-01T12:03:00Z".to_owned(), "2020-08-01T12:04:00Z".to_owned()],
        "the two most-recent instances across sources are projected post-aggregation"
    );

    topology.finish().await.expect("topology cleanup");
}
