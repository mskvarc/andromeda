//! Split-entity assembly, remote filter forwarding, unitary map caching, and map
//! expiry/location handling (ETSI GS CIM 009 v1.9.1, clauses 5.7.2, 5.14).
//! Remote-filter and map rows on `T-MOCK`; split assembly and the unitary
//! sequence on `T-SPLIT`/`T-CHAIN`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, typed_vehicle},
    oracle::{assert_status, entity_map_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// Locate the entity with the given id inside an entity-array response body.
fn find_entity<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()?.iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// With `splitEntities=false` the filter is forwarded to the source rather than
/// evaluated only locally: the `q` survives on the forwarded request (clause
/// 5.7.2). Confirms REQ-FW-53.
#[tokio::test]
async fn distop_entity_map_no_split_param_forwards_filters_remotely() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let vtype = topology.id("Vehicle");
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:map-nosplit"), &endpoint)
        .mode("inclusive")
        .entity_type(&vtype)
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
        .query_entities(
            &format!("type={}&q=speed%3E50&splitEntities=false", encode(&vtype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        forwarded_query.contains("speed"),
        "the q filter is forwarded to the source when not split: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A split entity is assembled from both sources before the local filter is
/// applied: with speed on one source and a supplementary attribute on another,
/// the assembled entity passes `q=speed>50` and carries both attributes. The
/// query sets `splitEntities=true` explicitly, since clause 5.7.2.4 applies the
/// assemble-then-filter path only "If the split entities flag is explicitly
/// set to true or, if not explicitly set, the default setting of the deployment
/// allows split entities" and the deployment default is complete entities,
/// under which `q` is forwarded and the fuel-only source returns nothing.
/// Confirms REQ-FW-54, REQ-FW-55.
#[tokio::test]
async fn distop_entity_map_split_entities_assembled_before_filtering() {
    let topology = topology::chain().await.expect("three-broker topology");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let speed_source = topology.broker(BrokerRole::B).expect("topology broker B");
    let fuel_source = topology.broker(BrokerRole::C).expect("topology broker C");
    let entity_id = topology.id("Vehicle:split-assemble");

    assert_status(
        &speed_source
            .client()
            .post_entity(&entity(&entity_id, &vtype, &[("speed", property(json!(60)))]))
            .with_ld_json()
            .send()
            .await
            .expect("speed source"),
        201,
    );
    assert_status(
        &fuel_source
            .client()
            .post_entity(&entity(&entity_id, &vtype, &[("fuel", property(json!(30)))]))
            .with_ld_json()
            .send()
            .await
            .expect("fuel source"),
        201,
    );

    let speed_registration = CsrBuilder::new(&topology.id("registration:split-speed"), speed_source.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&speed_registration).await.expect("speed registration"), 201);
    let fuel_registration = CsrBuilder::new(&topology.id("registration:split-fuel"), fuel_source.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .entity_id(&entity_id)
        .property_names(&["fuel"])
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&fuel_registration).await.expect("fuel registration"), 201);

    let response = front
        .client()
        .query_entities(
            &format!("type={}&id={entity_id}&q=speed%3E50&splitEntities=true", encode(&vtype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    let assembled = find_entity(&body, &entity_id).expect("the assembled entity passes the filter");
    assert_eq!(assembled["speed"]["value"].as_i64(), Some(60), "the speed source contributes speed");
    assert_eq!(assembled["fuel"]["value"].as_i64(), Some(30), "the fuel source is assembled before filtering");

    topology.finish().await.expect("topology cleanup");
}

/// A unitary sequence over a chain creates and caches an entity map, returning
/// its location (clause 5.7.2, 5.14). Confirms REQ-FW-71, REQ-FW-72.
#[tokio::test]
async fn distop_entity_map_unitary_sequence_creates_and_caches_map() {
    let topology = topology::chain().await.expect("three-broker topology");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let source = topology.broker(BrokerRole::B).expect("topology broker B");
    let entity_id = topology.id("Vehicle:unitary-map");
    assert_status(
        &source
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 45))
            .with_ld_json()
            .send()
            .await
            .expect("source create"),
        201,
    );

    let registration = CsrBuilder::new(&topology.id("registration:unitary-map"), source.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let created = front
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");
    assert_status(&front.client().retrieve_entity_map(&map_id).await.expect("retrieve cached map"), 200);

    topology.finish().await.expect("topology cleanup");
}

/// A suggested `entityMapLifetime` is honoured in the created map's `expiresAt`
/// (clause 5.14). Confirms REQ-FW-73.
#[tokio::test]
async fn distop_entity_map_suggested_expiry_honoured() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-expiry");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 15))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&entityMap=true&entityMapLifetime=PT1H", encode(&vtype)),
            "application/ld+json",
            None,
        )
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve map");
    assert_status(&retrieved, 200);
    assert!(
        json_body(&retrieved).get("expiresAt").is_some(),
        "the suggested lifetime is reflected in the map's expiresAt"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A source that declines to return a map location does not cause an error: the
/// broker proceeds and returns its own map (clause 5.7.2). Confirms REQ-FW-74.
#[tokio::test]
async fn distop_entity_map_source_may_decline_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:map-decline");
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([typed_vehicle(&entity_id, &vtype, 25)]))
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:map-decline"), &endpoint)
        .mode("inclusive")
        .entity_type(&vtype)
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

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    assert!(
        entity_map_header(&created).is_some(),
        "the broker returns its own map when the source declines a location"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
