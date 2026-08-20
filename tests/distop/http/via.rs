//! `Via` cascade support (clause 4.3.6.4 / 6.3.18): every forwarded request
//! carries a `Via` entry naming the forwarding broker, a second hop appends its
//! own host alias after the first, and an incoming `Via` naming a registration's
//! alias excludes that source from matching so a loop cannot form.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header, retrieval_source},
    oracle::{assert_status, via_pseudonyms},
    registration::CsrBuilder,
    topology::{BrokerRole, DistributedTopology},
};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// A single forwarded request to a source carries a `Via` header naming the
/// forwarding broker (clause 6.3.18). Confirms REQ-HTTP-37.
#[tokio::test]
async fn dist_http_forwarded_request_carries_via_header_on_all_distributed_resources() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:via-carried");
    let reg_id = topology.id("registration:via-carried");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert!(
        !via_pseudonyms(&forwarded).is_empty(),
        "the forwarded request must carry a Via pseudonym for the broker"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A two-hop forward (A -> B -> source) appends each broker's host alias in
/// order, so the source sees both with the nearer broker last (clause 4.3.6.4).
/// Confirms REQ-HTTP-38.
#[tokio::test]
async fn dist_http_chain_forward_appends_broker_host_alias_to_via() {
    let topology = DistributedTopology::chain().await.expect("three-broker topology");
    let front = topology.broker(BrokerRole::A);
    let middle = topology.broker(BrokerRole::B);
    let entity_id = topology.id("Vehicle:via-chain");
    let front_reg = topology.id("registration:via-chain-front");
    let middle_reg = topology.id("registration:via-chain-middle");
    let mock = ContextSourceAdapter::start().await;

    // Middle broker B forwards the covered entity to the mock tail.
    let to_mock = retrieval_source(&middle_reg, &mock.url(""), &entity_id, "inclusive");
    assert_status(&middle.client().create_registration(&to_mock).await.expect("register mock on middle"), 201);
    // Front broker A forwards to the real middle broker B.
    let to_middle = retrieval_source(&front_reg, middle.peer_url(), &entity_id, "inclusive");
    assert_status(&front.client().create_registration(&to_middle).await.expect("register middle on front"), 201);
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = front
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("chained query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(Duration::from_secs(10)).await;
    let pseudonyms = via_pseudonyms(&forwarded);
    assert!(
        pseudonyms.contains(&front.alias().to_owned()),
        "the Via must name the front broker, got {pseudonyms:?}"
    );
    assert!(
        pseudonyms.contains(&middle.alias().to_owned()),
        "the Via must name the middle broker, got {pseudonyms:?}"
    );
    assert_eq!(
        pseudonyms.last().map(String::as_str),
        Some(middle.alias()),
        "the nearest broker is appended last, got {pseudonyms:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An incoming `Via` naming a registration's own `contextSourceAlias` excludes
/// that source from matching, so it is not forwarded to (clause 4.3.6.4).
/// Confirms REQ-HTTP-39.
#[tokio::test]
async fn dist_http_incoming_via_excludes_listed_source_from_matching() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:via-excluded");
    let reg_id = topology.id("registration:via-excluded");
    let alias = "cs-mock-excluded";
    let mock = ContextSourceAdapter::start().await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity", "queryEntity", "queryBatch"])
        .alias(alias)
        .build();
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register aliased source"),
        201,
    );
    let one_vehicle: Value = json!([]);
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &one_vehicle).await;

    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("via", format!("1.1 {alias}").as_str())
        .send()
        .await
        .expect("query with Via header");
    assert_status(&response, 200);

    assert_eq!(
        mock.get_count(Method::GET, FORWARDED_ENTITIES_PATH).await,
        0,
        "a source listed in the incoming Via must be excluded from forwarding"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
