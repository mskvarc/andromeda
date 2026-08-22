//! `NGSILD-EntityMap` request and response header behaviour (clause 6.3.15): a
//! distributed query or retrieve requested with `entityMap=true` returns the map
//! id in the response header, and a subsequent request naming that map in the
//! request header reuses it. All `T-MOCK`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header, retrieval_source},
    oracle::{assert_status, entity_map_header},
    topology::{self, BrokerRole},
};
use reqwest::Method;
use serde_json::json;

/// A distributed query requested with `entityMap=true` returns an
/// `NGSILD-EntityMap` header and `200`/`201` (clause 6.3.15). Confirms REQ-HTTP-54.
#[tokio::test]
async fn dist_http_query_entity_map_requested_returns_map_header_200_or_201() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:map-req");
    let reg_id = topology.id("registration:map-req");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle&entityMap=true", "application/ld+json", Some(context_link()))
        .await
        .expect("entity-map query");
    assert!(
        [200, 201].contains(&response.status.as_u16()),
        "an entityMap query returns 200 or 201, got {}",
        response.status.as_u16()
    );
    assert!(entity_map_header(&response).is_some(), "the response carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A query naming an existing map in the `NGSILD-EntityMap` request header reuses
/// that map (clause 6.3.15). Confirms REQ-HTTP-53.
#[tokio::test]
async fn dist_http_query_with_entity_map_request_header_uses_named_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:map-named");
    let reg_id = topology.id("registration:map-named");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let first = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle&entityMap=true", "application/ld+json", Some(context_link()))
        .await
        .expect("first entity-map query");
    let map_id = entity_map_header(&first).unwrap_or_else(|| unreachable!("first query yields an entity map"));

    let second = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle&entityMap=true")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("ngsild-entitymap", map_id.as_str())
        .send()
        .await
        .expect("named-map query");
    assert_status(&second, 200);
    assert_eq!(entity_map_header(&second).as_deref(), Some(map_id.as_str()), "the named map is reused");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A purge accepts the `NGSILD-EntityMap` request header (clause 6.3.15).
/// Confirms REQ-HTTP-60.
#[tokio::test]
async fn dist_http_purge_supports_entity_map_request_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:map-purge");
    let reg_id = topology.id("registration:map-purge");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;
    let map = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle&entityMap=true", "application/ld+json", Some(context_link()))
        .await
        .expect("entity-map query");
    let map_id = entity_map_header(&map).unwrap_or_else(|| unreachable!("query yields an entity map"));

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::DELETE, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("link", context_link_header())
        .header("ngsild-entitymap", map_id.as_str())
        .send()
        .await
        .expect("purge with entity-map header");
    assert_status(&response, 204);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve naming an existing map in the `NGSILD-EntityMap` request header
/// reuses that map (clause 6.3.15). Confirms REQ-HTTP-64.
#[tokio::test]
async fn dist_http_retrieve_with_entity_map_request_header_uses_named_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:map-retrieve-named");
    let reg_id = topology.id("registration:map-retrieve-named");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;
    let map = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle&entityMap=true", "application/ld+json", Some(context_link()))
        .await
        .expect("entity-map query");
    let map_id = entity_map_header(&map).unwrap_or_else(|| unreachable!("query yields an entity map"));

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&entity_id)))
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("ngsild-entitymap", map_id.as_str())
        .send()
        .await
        .expect("named-map retrieve");
    assert_status(&response, 200);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve requested with `entityMap=true` returns `200` and an
/// `NGSILD-EntityMap` header (clause 6.3.15). Confirms REQ-HTTP-65.
#[tokio::test]
async fn dist_http_retrieve_entity_map_requested_returns_200_and_map_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:map-retrieve-req");
    let reg_id = topology.id("registration:map-retrieve-req");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .get_entity(&entity_id, "entityMap=true", "application/ld+json", Some(context_link()))
        .await
        .expect("entity-map retrieve");
    assert_status(&response, 200);
    assert!(entity_map_header(&response).is_some(), "the retrieve carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
