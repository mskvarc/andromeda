//! Inclusive mode: a source supplements the broker's own data, both the local
//! copy and every matching source contributing to the result, all sources of
//! equal priority (ETSI GS CIM 009 v1.9.1, clause 4.3.6.3, 5.7). Real-peer
//! integration on `T-PAIR`; forwarded-request capture on `T-MOCK`/`T-MOCK-N`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, entity, property, vehicle},
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// Locate the entity with the given id inside an entity-array response body.
fn find_entity<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()?.iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// An inclusive registration distributes a retrieve to the source even when the
/// broker already holds part of the entity, integrating the local attribute and
/// the source attribute into one entity (clause 4.3.6.3). Confirms REQ-FW-12.
#[tokio::test]
async fn distop_entity_inclusive_distributes_despite_local_data_and_integrates() {
    let topology = topology::pair().await.expect("two-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let entity_id = topology.id("Vehicle:inclusive-merge");

    let local = entity(&entity_id, "Vehicle", &[("brandName", property(json!("Mercedes")))]);
    assert_status(&front.client().post_entity(&local).with_ld_json().send().await.expect("local create"), 201);
    let remote = entity(&entity_id, "Vehicle", &[("fuel", property(json!(30)))]);
    assert_status(&peer.client().post_entity(&remote).with_ld_json().send().await.expect("peer create"), 201);

    let registration = CsrBuilder::new(&topology.id("registration:inclusive-merge"), peer.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["fuel"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let retrieved = front
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("front retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(body["brandName"]["value"].as_str(), Some("Mercedes"), "the local attribute is integrated");
    assert_eq!(body["fuel"]["value"].as_i64(), Some(30), "the source attribute is integrated");

    topology.finish().await.expect("topology cleanup");
}

/// An inclusive additive provision is serviced locally and distributed to the
/// source: the append reaches the source and the new attribute is also held
/// locally (clause 4.3.6.3). Confirms REQ-FW-11.
#[tokio::test]
async fn distop_entity_inclusive_additive_provision_serviced_locally_and_distributed() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:inclusive-append");
    mock.set_stub_with_body(Method::POST, &format!("{FORWARDED_ENTITIES}/{entity_id}/attrs"), 204, &json!({}))
        .await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 20))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:inclusive-append"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["fuel"])
        .operations(&["appendAttrs"])
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

    let fragment = attribute_fragment(&[("fuel", property(json!(45)))]);
    let appended = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .append_entity_attrs(&entity_id, &fragment)
        .await
        .expect("append attrs");
    assert!(appended.status.is_success(), "the append succeeds: {}", appended.status);

    assert_eq!(mock.get_total_count(), 1, "the append is distributed to the source");
    let local = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local retrieve");
    assert_status(&local, 200);
    assert_eq!(
        json_body(&local)["fuel"]["value"].as_i64(),
        Some(45),
        "the appended attribute is held locally too"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Two inclusive sources covering overlapping data each receive one forwarded
/// query and both contribute to the integrated result, of equal priority
/// (clause 4.3.6.3). Confirms REQ-FW-13, REQ-FW-46.
#[tokio::test]
async fn distop_entity_inclusive_multiple_sources_equal_priority() {
    let topology = topology::single().await.expect("single-broker topology");
    let first_mock = ContextSourceAdapter::start().await;
    let second_mock = ContextSourceAdapter::start().await;

    let first_id = topology.id("Vehicle:inclusive-multi-1");
    let second_id = topology.id("Vehicle:inclusive-multi-2");
    first_mock
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([vehicle(&first_id, 11)]))
        .await;
    second_mock
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([vehicle(&second_id, 22)]))
        .await;

    let first_endpoint = first_mock.url("");
    let second_endpoint = second_mock.url("");
    for (suffix, endpoint) in [("1", &first_endpoint), ("2", &second_endpoint)] {
        let registration = CsrBuilder::new(&topology.id(&format!("registration:inclusive-multi-{suffix}")), endpoint)
            .mode("inclusive")
            .entity_type("Vehicle")
            .operations(&["queryEntity"])
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
    }

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(find_entity(&body, &first_id).is_some(), "the first source contributes its entity: {body}");
    assert!(find_entity(&body, &second_id).is_some(), "the second source contributes its entity: {body}");
    assert_eq!(
        first_mock.get_count(Method::GET, FORWARDED_ENTITIES),
        1,
        "the first source is queried exactly once"
    );
    assert_eq!(
        second_mock.get_count(Method::GET, FORWARDED_ENTITIES),
        1,
        "the second source is queried exactly once"
    );

    first_mock.shutdown().await;
    second_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A registration with `mode` omitted behaves as inclusive: the source is
/// queried and the local data is returned alongside it (clause 5.2.9 default,
/// 4.3.6.3). Confirms REQ-FW-14.
#[tokio::test]
async fn distop_entity_inclusive_is_default_mode_when_unspecified() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:inclusive-default");
    mock.set_stub_with_body(
        Method::GET,
        FORWARDED_ENTITIES,
        200,
        &json!([entity(&entity_id, "Vehicle", &[("fuel", property(json!(60)))])]),
    )
    .await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 10))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:inclusive-default"), &endpoint)
        .entity_id(&entity_id)
        .property_names(&["fuel"])
        .operations(&["queryEntity"])
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type=Vehicle&id={entity_id}"), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    let merged = find_entity(&body, &entity_id).expect("the entity is present");
    assert_eq!(merged["speed"]["value"].as_i64(), Some(10), "the local data is returned");
    assert_eq!(
        merged["fuel"]["value"].as_i64(),
        Some(60),
        "the source data is returned as under inclusive mode"
    );
    assert_eq!(mock.get_count(Method::GET, FORWARDED_ENTITIES), 1, "an omitted mode still forwards the query");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
