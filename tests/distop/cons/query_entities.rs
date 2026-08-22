//! Query Entities forwarding (clause 5.7.2.4): request-width and `orderBy`
//! validation, capability/mode-based forwarding selection, expired-entity
//! discard, and the 4.5.5 array merge. `datasetId`, pagination, csf, auxiliary,
//! and local ordering rows live in `crate::cons::query_entities_filters`;
//! split-filter timing in `crate::cons::split_entities`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use axum::http::Method;
use serde_json::{Value, json};

/// The `id` strings of every entity in an Entity Array response body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// A query with none of type/attrs/q/geoQ/local is too wide and rejected with
/// `400 BadRequestData` before any forwarding (clause 5.7.2.4). Confirms
/// REQ-CONS-19.
#[tokio::test]
async fn distop_query_entities_too_wide_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons19");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front.client().query_entities("", "application/ld+json", None).await.expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    assert_eq!(mock.get_total_count().await, 0, "a rejected too-wide query forwards nothing");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// `orderBy` requires `local=true`: without it the query is rejected `400`; with
/// it the same `orderBy` succeeds (clause 5.7.2.4). Confirms REQ-CONS-20.
#[tokio::test]
async fn distop_query_entities_orderby_without_local_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons20");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let rejected = front
        .client()
        .query_entities("type=Vehicle&orderBy=speed", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);
    assert_eq!(mock.get_total_count().await, 0, "the rejected query forwards nothing");

    let accepted = front
        .client()
        .query_entities("type=Vehicle&orderBy=speed&local=true", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&accepted, 200);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-local query forwards only to `queryEntity`-capable sources; a `local=true`
/// query forwards to none (clause 5.7.2.4). Confirms REQ-CONS-25.
#[tokio::test]
async fn distop_query_entities_forwards_matching_queryentity_csrs_local_none() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    for (source, operations) in [("/s1", &["queryEntity"][..]), ("/s2", &["retrieveEntity"][..])] {
        let registration_id = topology.id(&format!("registration:cons25-{}", source.trim_start_matches('/')));
        let registration = CsrBuilder::new(&registration_id, &mock.url(source))
            .mode("inclusive")
            .operations(operations)
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities"), 200, &json!([]))
            .await;
    }

    assert_status(
        &front.client().query_entities("type=Vehicle", "application/ld+json", None).await.expect("query"),
        200,
    );
    assert!(
        mock.get_count(Method::GET, "/s1/ngsi-ld/v1/entities").await > 0,
        "queryEntity source is forwarded"
    );
    assert_eq!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/entities").await,
        0,
        "retrieveEntity-only source is not forwarded"
    );

    let total_before_local = mock.get_total_count().await;
    assert_status(
        &front
            .client()
            .query_entities("type=Vehicle&local=true", "application/ld+json", None)
            .await
            .expect("query"),
        200,
    );
    assert_eq!(mock.get_total_count().await, total_before_local, "a local=true query forwards to no source");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Exclusive, redirect, and inclusive sources each return an Entity Array, and the
/// aggregated response contains entities from all three (clause 5.7.2.4). The
/// exclusive registration names its entity id and attributes, as clause 4.3.6.3
/// requires ("An exclusive registration shall always relate to specific
/// Attributes found on a single Entity. Thus, the registration shall define
/// both: An entity id ... Attributes."). Confirms REQ-CONS-29.
#[tokio::test]
async fn distop_query_entities_forwards_all_modes_returns_entity_arrays() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let mut expected = Vec::new();
    for mode in ["exclusive", "redirect", "inclusive"] {
        let source = format!("/{mode}");
        let entity_id = topology.id(&format!("Vehicle:cons29-{mode}"));
        let registration_id = topology.id(&format!("registration:cons29-{mode}"));
        let builder = CsrBuilder::new(&registration_id, &mock.url(&source)).mode(mode).operations(&["queryEntity"]);
        let registration = match mode {
            "exclusive" => builder.entity_id(&entity_id).property_names(&["speed"]).build(),
            "redirect" | "inclusive" => builder.build(),
            other => unreachable!("the mode loop names only the three proxying modes, got {other}"),
        };
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
        mock.set_stub_with_body(Method::GET, &format!("{source}/ngsi-ld/v1/entities"), 200, &json!([vehicle(&entity_id, 50)]))
            .await;
        expected.push(entity_id);
    }

    let response = front.client().query_entities("type=Vehicle", "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);
    let returned = ids(&json_body(&response));
    for entity_id in &expected {
        assert!(returned.contains(entity_id), "aggregated array must contain {entity_id}, got {returned:?}");
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Entities whose attributes have all expired are discarded from a remote Entity
/// Array before aggregation (clause 5.7.2.4). Confirms REQ-CONS-30.
#[tokio::test]
async fn distop_query_entities_discards_expired_entities_in_remote_array() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let expired_id = topology.id("Vehicle:cons30-expired");
    let valid_id = topology.id("Vehicle:cons30-valid");
    let registration_id = topology.id("registration:cons30");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let expired = entity(
        &expired_id,
        "Vehicle",
        &[("speed", json!({ "type": "Property", "value": 90, "expiresAt": "2000-01-01T00:00:00Z" }))],
    );
    let array = json!([expired, vehicle(&valid_id, 50)]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &array).await;

    let response = front.client().query_entities("type=Vehicle", "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);
    let returned = ids(&json_body(&response));
    assert!(returned.contains(&valid_id), "the non-expired entity is returned: {returned:?}");
    assert!(!returned.contains(&expired_id), "the fully-expired entity is discarded: {returned:?}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Per-source Entity Arrays merge per clause 4.5.5: a same-id entity is merged
/// across sources, and distinct-id entities coexist. Confirms REQ-CONS-31.
#[tokio::test]
async fn distop_query_entities_merges_arrays_per_4_5_5() {
    let topology = topology::chain().await.expect("three-broker topology");
    let shared_id = topology.id("Vehicle:cons31-shared");
    let extra_id = topology.id("Vehicle:cons31-extra");
    let front = topology.broker(BrokerRole::A);
    let holder_b = topology.broker(BrokerRole::B);
    let holder_c = topology.broker(BrokerRole::C);

    assert_status(
        &front
            .client()
            .post_entity(&entity(&shared_id, "Vehicle", &[("fuelLevel", property(json!(70)))]))
            .with_ld_json()
            .send()
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .post_entity(&entity(&shared_id, "Vehicle", &[("speed", property(json!(50)))]))
            .with_ld_json()
            .send()
            .await
            .expect("b shared"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&entity(
                &shared_id,
                "Vehicle",
                &[(
                    "location",
                    json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } }),
                )],
            ))
            .with_ld_json()
            .send()
            .await
            .expect("c shared"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .post_entity(&vehicle(&extra_id, 40))
            .with_ld_json()
            .send()
            .await
            .expect("c extra"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role);
        let registration_id = topology.id(&format!("registration:cons31-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .mode("inclusive")
            .operations(&["queryEntity"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let response = front.client().query_entities("type=Vehicle", "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(ids(&body).contains(&extra_id), "the distinct-id entity is present: {body}");
    let merged = body
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(shared_id.as_str())))
        .cloned()
        .unwrap_or(Value::Null);
    assert!(
        merged.get("speed").is_some() && merged.get("location").is_some() && merged.get("fuelLevel").is_some(),
        "the same-id entity merges all three sources: {merged}"
    );

    topology.finish().await.expect("topology cleanup");
}
