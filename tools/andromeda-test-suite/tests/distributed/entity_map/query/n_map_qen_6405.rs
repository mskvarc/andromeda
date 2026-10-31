//! A map unknown to another broker is recreated by it.

use andromeda_test_suite::{broker, config::BrokerRole, header_matcher::present, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::Entity,
};

/// Pins: Query Entities by type naming an `EntityMap` built on broker A answers 200 with the same map on broker A,
/// and 201 with a new map on peer broker B, which does not know it
/// Clause: 5.5.14, 6.4.3.2, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_map_qen_6405_a_map_unknown_to_another_broker_is_recreated_by_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 18)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    let page_2 = headed.outcome;
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    expect_that!(page_2.items(), len(eq(1)));
    let entity_map_request_3 = EntityMapRequest::reuse(map.clone());
    let page_3 = broker_b
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request_3)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page_3.entity_map(), present());
    expect_that!(
        page_3.entity_map().valid().map(|map| map.as_iri_ref().as_str()),
        not(some(eq(map.as_iri_ref().as_str())))
    );
    Ok(())
}
