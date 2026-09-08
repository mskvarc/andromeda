//! Retrieve Entity auxiliary supplementation, shorthand term expansion, and
//! post-merge `datasetId` / attribute projection (clauses 5.7.1.4, 5.5.7). The
//! forwarding-selection and 4.5.5 merge rows live in `crate::cons::retrieve_entity`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, property_with_dataset, vehicle},
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::RESOURCE_NOT_FOUND;
use axum::http::Method;
use serde_json::json;
use std::time::Duration;
use urlencoding::encode;

/// An auxiliary source supplies an attribute only where it is absent from the
/// merged result: it does not override a present local attribute, but it does add
/// one that is missing (clause 5.7.1.4, auxiliary mode). Confirms REQ-CONS-12.
#[tokio::test]
async fn distop_retrieve_entity_auxiliary_added_only_when_absent() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons12");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front.client().post_entity(&vehicle(&entity_id, 42)).with_ld_json().send().await.expect("local"),
        201,
    );
    let auxiliary = entity(&entity_id, "Vehicle", &[("speed", property(json!(99))), ("fuelLevel", property(json!(70)))]);
    assert_status(&peer.client().post_entity(&auxiliary).with_ld_json().send().await.expect("peer"), 201);

    let registration_id = topology.id("registration:cons12");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .mode("auxiliary")
        .entity_id(&entity_id)
        .property_names(&["speed", "fuelLevel"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(
        body["speed"]["value"].as_i64(),
        Some(42),
        "auxiliary speed must not override the present local speed"
    );
    assert!(body.get("fuelLevel").is_some(), "auxiliary fuelLevel is added because it was absent: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// A shorthand `attrs` term is expanded against the request `@context` before
/// registration matching and forwarding, so an exclusive CSR on the expanded IRI
/// matches and the forwarded request carries the projected attribute
/// (clause 5.5.7). Confirms REQ-CONS-14.
#[tokio::test]
async fn distop_retrieve_entity_expands_shorthand_attribute_names() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons14");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons14");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 55))
        .await;

    let retrieved = front
        .client()
        .get_entity(&entity_id, "attrs=speed", "application/ld+json", None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);

    let forwarded = mock.next_request(Duration::from_secs(2)).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(
        query.contains("attrs"),
        "the expanded projection is forwarded to the source, got query {query:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `datasetId` query parameter keeps only the matching instances post-merge;
/// `@none` keeps only the default instance (clause 5.7.1.4). Confirms REQ-CONS-15.
#[tokio::test]
async fn distop_retrieve_entity_datasetid_keeps_matching_instances() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons15");
    let dataset_id = topology.id("dataset:2");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front.client().post_entity(&vehicle(&entity_id, 10)).with_ld_json().send().await.expect("local"),
        201,
    );
    let remote = entity(&entity_id, "Vehicle", &[("speed", property_with_dataset(json!(30), &dataset_id))]);
    assert_status(&holder.client().post_entity(&remote).with_ld_json().send().await.expect("holder"), 201);

    let registration_id = topology.id("registration:cons15");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let by_dataset = front
        .client()
        .get_entity(&entity_id, &format!("datasetId={}", encode(&dataset_id)), "application/ld+json", None)
        .await
        .expect("dataset");
    assert_status(&by_dataset, 200);
    let body = json_body(&by_dataset);
    let instances = body["speed"].as_array().cloned().unwrap_or_else(|| vec![body["speed"].clone()]);
    assert!(
        instances.iter().all(|instance| instance["datasetId"].as_str() == Some(dataset_id.as_str())),
        "only the requested-datasetId instance is kept: {body}"
    );

    let by_none = front
        .client()
        .get_entity(&entity_id, "datasetId=@none", "application/ld+json", None)
        .await
        .expect("none");
    assert_status(&by_none, 200);
    let none_body = json_body(&by_none);
    let none_instances = none_body["speed"].as_array().cloned().unwrap_or_else(|| vec![none_body["speed"].clone()]);
    assert!(
        none_instances.iter().all(|instance| instance.get("datasetId").is_none()),
        "@none keeps only the default instance: {none_body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A `datasetId` that no instance of an attribute carries omits that attribute
/// from the retrieved entity after aggregation (clause 5.7.1.4). Confirms
/// REQ-CONS-16.
#[tokio::test]
async fn distop_retrieve_entity_datasetid_no_match_omits_attribute() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons16");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder = topology.broker(BrokerRole::C).expect("topology broker C");

    assert_status(
        &front.client().post_entity(&vehicle(&entity_id, 10)).with_ld_json().send().await.expect("local"),
        201,
    );
    let remote = entity(
        &entity_id,
        "Vehicle",
        &[(
            "location",
            json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [1.0, 2.0] } }),
        )],
    );
    assert_status(&holder.client().post_entity(&remote).with_ld_json().send().await.expect("holder"), 201);

    let registration_id = topology.id("registration:cons16");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["location"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let absent = topology.id("dataset:absent");
    let retrieved = front
        .client()
        .get_entity(&entity_id, &format!("datasetId={}", encode(&absent)), "application/ld+json", None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    assert!(
        json_body(&retrieved).get("speed").is_none(),
        "speed carries no matching datasetId instance and is omitted"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A projection that no holder can satisfy yields `404 ResourceNotFound`, but the
/// decision is taken after distributed aggregation: a projection only a remote
/// holder satisfies returns `200` (clause 5.7.1.4). Confirms REQ-CONS-17.
#[tokio::test]
async fn distop_retrieve_entity_projection_lacking_all_attrs_404_after_aggregation() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons17");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front.client().post_entity(&vehicle(&entity_id, 42)).with_ld_json().send().await.expect("local"),
        201,
    );
    let remote = entity(&entity_id, "Vehicle", &[("fuelLevel", property(json!(70)))]);
    assert_status(&peer.client().post_entity(&remote).with_ld_json().send().await.expect("peer"), 201);

    let registration_id = topology.id("registration:cons17");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["fuelLevel"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let missing = front
        .client()
        .get_entity(&entity_id, "attrs=color", "application/ld+json", None)
        .await
        .expect("missing");
    assert_status(&missing, 404);
    assert_problem_details(&missing, RESOURCE_NOT_FOUND);

    let remote_only = front
        .client()
        .get_entity(&entity_id, "attrs=fuelLevel", "application/ld+json", None)
        .await
        .expect("remote-only");
    assert_status(&remote_only, 200);
    assert!(
        json_body(&remote_only).get("fuelLevel").is_some(),
        "a projection only the remote holder satisfies returns 200 after aggregation"
    );

    topology.finish().await.expect("topology cleanup");
}
