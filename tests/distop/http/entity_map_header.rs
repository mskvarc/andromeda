//! `NGSILD-EntityMap` request and response header behaviour (clause 6.3.15): a distributed query or retrieve requested with `entityMap=true` returns the map id in the response header, and a subsequent request naming that map in the request header reuses it.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::typed_vehicle,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header, retrieval_source},
    oracle::{assert_status, entity_map_header, json_body},
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::json;

/// A distributed query requested with `entityMap=true` returns an `NGSILD-EntityMap` header and `200`/`201` (clause 6.3.15).
#[tokio::test]
async fn dist_http_query_entity_map_requested_returns_map_header_200_or_201() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-req");
    let reg_id = topology.id("registration:map-req");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&entityMap=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
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

/// A query naming an existing map in the `NGSILD-EntityMap` request header reuses that map (clause 6.3.15).
#[tokio::test]
async fn dist_http_query_with_entity_map_request_header_uses_named_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-named");
    let reg_id = topology.id("registration:map-named");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let first = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&entityMap=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("first entity-map query");
    let map_id = entity_map_header(&first).unwrap_or_else(|| panic!("first query yields an entity map"));

    let second = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(
            Method::GET,
            &format!("/ngsi-ld/v1/entities?type={}&entityMap=true", urlencoding::encode(&vtype)),
        )
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

/// A purge under an `NGSILD-EntityMap` request header determines its entity set from the map and returns the map's location (Table 6.4.3.3-2: "If present, the `EntityMap` supplied is used for determining the set of Entities requested during the purge operation. The location of the `EntityMap` used in the purge operation is returned in the response").
#[tokio::test]
async fn dist_http_purge_supports_entity_map_request_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mapped_id = topology.id("Vehicle:map-purge-mapped");
    let unmapped_id = topology.id("Vehicle:map-purge-unmapped");
    let reg_id = topology.id("registration:map-purge");
    let mock = ContextSourceAdapter::start().await;
    let broker = topology.broker(BrokerRole::A).expect("topology broker A");

    assert_status(
        &broker
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &mapped_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;
    for id in [&mapped_id, &unmapped_id] {
        assert_status(
            &broker
                .client()
                .post_entity(&typed_vehicle(id, &vtype, 5))
                .with_ld_json()
                .send()
                .await
                .expect("local create"),
            201,
        );
    }

    // The map fixes one of the two vehicles, so a purge held to it must leave
    // the other alone even though the purge's own query matches both.
    let map = broker
        .client()
        .query_entities(
            &format!("type={}&id={}&entityMap=true", urlencoding::encode(&vtype), urlencoding::encode(&mapped_id)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("entity-map query");
    let map_id = entity_map_header(&map).unwrap_or_else(|| panic!("query yields an entity map"));

    let both = format!("{},{}", urlencoding::encode(&mapped_id), urlencoding::encode(&unmapped_id));
    let response = broker
        .client()
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/entities?type={}&id={both}", urlencoding::encode(&vtype)))
        .header("link", context_link_header())
        .header("ngsild-entitymap", map_id.as_str())
        .send()
        .await
        .expect("purge with entity-map header");
    assert_status(&response, 204);
    assert_eq!(
        entity_map_header(&response).as_deref(),
        Some(map_id.as_str()),
        "the purge answers with the location of the EntityMap it used"
    );

    let survivors = broker
        .client()
        .query_entities(
            &format!("type={}&id={both}&local=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("survivor query");
    assert_status(&survivors, 200);
    let surviving_ids: Vec<String> = json_body(&survivors)
        .as_array()
        .unwrap_or_else(|| panic!("entity query returns an array"))
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        surviving_ids,
        vec![unmapped_id],
        "only the entity the EntityMap listed is purged, got {surviving_ids:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve naming an existing map in the `NGSILD-EntityMap` request header reuses that map (Table 6.5.3.1-2: "If present, the `EntityMap` supplied is used for determining the extent of the Entity data requested during the retrieval operation. The location of the `EntityMap` used in the retrieval operation is returned in the response").
///
/// The returned location is what makes the named map observable: a retrieve that omits the header answers `200` with no `NGSILD-EntityMap` header at all, and one that names a map the broker does not honour answers `200` carrying a freshly minted map id instead of this one.
#[tokio::test]
async fn dist_http_retrieve_with_entity_map_request_header_uses_named_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-retrieve-named");
    let reg_id = topology.id("registration:map-retrieve-named");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;
    let map = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&entityMap=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("entity-map query");
    let map_id = entity_map_header(&map).unwrap_or_else(|| panic!("query yields an entity map"));

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&entity_id)))
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("ngsild-entitymap", map_id.as_str())
        .send()
        .await
        .expect("named-map retrieve");
    assert_status(&response, 200);
    assert_eq!(
        entity_map_header(&response).as_deref(),
        Some(map_id.as_str()),
        "the retrieve answers with the location of the EntityMap it used"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve requested with `entityMap=true` returns `200` and an `NGSILD-EntityMap` header (clause 6.3.15).
#[tokio::test]
async fn dist_http_retrieve_entity_map_requested_returns_200_and_map_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-retrieve-req");
    let reg_id = topology.id("registration:map-retrieve-req");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "entityMap=true", "application/ld+json", Some(context_link()))
        .await
        .expect("entity-map retrieve");
    assert_status(&response, 200);
    assert!(entity_map_header(&response).is_some(), "the retrieve carries an NGSILD-EntityMap header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
