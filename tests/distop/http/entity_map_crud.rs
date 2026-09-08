//! The `entityMaps` resource lifecycle (clause 6.3.15, 6.31–6.35): retrieving,
//! patching the expiry of, and deleting a map by id, plus the collection
//! operations that create a map and return its id in the `NGSILD-EntityMap`
//! header, for both the entity and temporal-entity variants. All `T-MOCK`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header, retrieval_source},
    oracle::{assert_status, entity_map_header},
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use reqwest::Method;
use serde_json::json;

/// Provision an entity map by running a distributed `entityMap=true` query
/// against a registered mock source, returning the created map id.
async fn provision_map(topology: &MultiBrokerTopology<BrokerRole>, mock: &ContextSourceAdapter, tag: &str) -> String {
    let entity_id = topology.id(&format!("Vehicle:{tag}"));
    let reg_id = topology.id(&format!("registration:{tag}"));
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
    entity_map_header(&response).unwrap_or_else(|| unreachable!("an entityMap query yields a map id"))
}

/// Retrieving an entity map by id returns `200` (clause 6.31). Confirms REQ-HTTP-93.
#[tokio::test]
async fn dist_http_get_entity_map_by_id_returns_200() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let map_id = provision_map(&topology, &mock, "map-get").await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve entity map");
    assert_status(&response, 200);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid entity map id on retrieve is rejected with `400` (clause 6.31).
/// Confirms REQ-HTTP-93.
#[tokio::test]
async fn dist_http_get_entity_map_by_id_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map("not a uri")
        .await
        .expect("invalid entity map retrieve");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// An unknown entity map id on retrieve returns `404` (clause 6.31).
/// Confirms REQ-HTTP-93.
#[tokio::test]
async fn dist_http_get_entity_map_by_id_unknown_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = topology.id("EntityMap:unknown-get");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("unknown entity map retrieve");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// Patching an entity map expiry returns `204` (clause 6.32). Confirms REQ-HTTP-94.
#[tokio::test]
async fn dist_http_patch_entity_map_expiry_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let map_id = provision_map(&topology, &mock, "map-patch").await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/entityMaps/{}", urlencoding::encode(&map_id)))
        .header("content-type", "application/json")
        .json(&json!({ "expiresAt": "2035-01-01T00:00:00Z" }))
        .send()
        .await
        .expect("patch entity map");
    assert_status(&response, 204);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid entity map patch body is rejected with `400` (clause 6.32).
/// Confirms REQ-HTTP-94.
#[tokio::test]
async fn dist_http_patch_entity_map_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let map_id = provision_map(&topology, &mock, "map-patch-400").await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/entityMaps/{}", urlencoding::encode(&map_id)))
        .header("content-type", "application/json")
        .json(&json!({ "expiresAt": "not-a-datetime" }))
        .send()
        .await
        .expect("invalid patch entity map");
    assert_status(&response, 400);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Patching an unknown entity map returns `404` (clause 6.32). Confirms REQ-HTTP-94.
#[tokio::test]
async fn dist_http_patch_entity_map_unknown_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = topology.id("EntityMap:unknown-patch");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/entityMaps/{}", urlencoding::encode(&map_id)))
        .header("content-type", "application/json")
        .json(&json!({ "expiresAt": "2035-01-01T00:00:00Z" }))
        .send()
        .await
        .expect("unknown patch entity map");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an entity map returns `204` (clause 6.33). Confirms REQ-HTTP-95.
#[tokio::test]
async fn dist_http_delete_entity_map_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let map_id = provision_map(&topology, &mock, "map-delete").await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .delete_entity_map(&map_id)
        .await
        .expect("delete entity map");
    assert_status(&response, 204);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid entity map id on delete is rejected with `400` (clause 6.33).
/// Confirms REQ-HTTP-95.
#[tokio::test]
async fn dist_http_delete_entity_map_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .delete_entity_map("not a uri")
        .await
        .expect("invalid entity map delete");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// An unknown entity map id on delete returns `404` (clause 6.33).
/// Confirms REQ-HTTP-95.
#[tokio::test]
async fn dist_http_delete_entity_map_unknown_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = topology.id("EntityMap:unknown-delete");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .delete_entity_map(&map_id)
        .await
        .expect("unknown entity map delete");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// The `entityMaps` collection GET creates a map and returns its id in the header
/// (clause 6.31). Confirms REQ-HTTP-98.
#[tokio::test]
async fn dist_http_get_entity_maps_create_returns_201_and_map_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:maps-get-create");
    let reg_id = topology.id("registration:maps-get-create");
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
        .raw(Method::GET, "/ngsi-ld/v1/entityMaps?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .send()
        .await
        .expect("entityMaps collection get");
    assert!(
        [200, 201].contains(&response.status.as_u16()),
        "the collection get creates a map (200/201), got {}",
        response.status.as_u16()
    );
    assert!(entity_map_header(&response).is_some(), "the response carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid `entityMaps` collection request is rejected with `400`
/// (clause 6.31). Confirms REQ-HTTP-100.
#[tokio::test]
async fn dist_http_get_entity_maps_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entityMaps?type=Vehicle&q=speed%3E")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .send()
        .await
        .expect("invalid entityMaps collection get");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// The `entityMaps` collection POST with a Query body creates a map and returns
/// its id in the header (clause 6.31). Confirms REQ-HTTP-101.
#[tokio::test]
async fn dist_http_post_entity_maps_query_body_returns_201_and_map_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:maps-post");
    let reg_id = topology.id("registration:maps-post");
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
        .raw(Method::POST, "/ngsi-ld/v1/entityMaps")
        .header("content-type", "application/ld+json")
        .header("accept", "application/ld+json")
        .json(&json!({ "type": "Query", "entities": [ { "type": "Vehicle" } ], "@context": context_link() }))
        .send()
        .await
        .expect("entityMaps collection post");
    assert!(
        [200, 201].contains(&response.status.as_u16()),
        "the collection post creates a map (200/201), got {}",
        response.status.as_u16()
    );
    assert!(entity_map_header(&response).is_some(), "the response carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The temporal `entityMaps` collection GET creates a map and returns its id in
/// the header (clause 6.34). The request carries a temporal query because
/// clause 5.14.5.4 opens with "If a temporal query is not provided then an error
/// of type BadRequestData shall be raised". Confirms REQ-HTTP-102.
#[tokio::test]
async fn dist_http_get_temporal_entity_maps_create_returns_201_and_map_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:tmaps-get");
    let reg_id = topology.id("registration:tmaps-get");
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
        .raw(
            Method::GET,
            "/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&timerel=before&timeAt=2022-01-01T00:00:00Z",
        )
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .send()
        .await
        .expect("temporal entityMaps collection get");
    assert!(
        [200, 201].contains(&response.status.as_u16()),
        "the temporal collection get creates a map (200/201), got {}",
        response.status.as_u16()
    );
    assert!(entity_map_header(&response).is_some(), "the response carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The temporal `entityMaps` collection POST with a Query body creates a map and
/// returns its id in the header (clause 6.34). The Query carries a `temporalQ`
/// because clause 5.14.5.4 opens with "If a temporal query is not provided then
/// an error of type BadRequestData shall be raised". Confirms REQ-HTTP-103.
#[tokio::test]
async fn dist_http_post_temporal_entity_maps_query_body_returns_201_and_map_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:tmaps-post");
    let reg_id = topology.id("registration:tmaps-post");
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
        .raw(Method::POST, "/ngsi-ld/v1/temporal/entityMaps")
        .header("content-type", "application/ld+json")
        .header("accept", "application/ld+json")
        .json(&json!({
            "type": "Query",
            "entities": [ { "type": "Vehicle" } ],
            "temporalQ": { "timerel": "before", "timeAt": "2022-01-01T00:00:00Z" },
            "@context": context_link()
        }))
        .send()
        .await
        .expect("temporal entityMaps collection post");
    assert!(
        [200, 201].contains(&response.status.as_u16()),
        "the temporal collection post creates a map (200/201), got {}",
        response.status.as_u16()
    );
    assert!(entity_map_header(&response).is_some(), "the response carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
