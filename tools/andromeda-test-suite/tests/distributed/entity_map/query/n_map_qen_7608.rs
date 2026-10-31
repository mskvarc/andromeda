//! A named map is reused and named again.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::Entity,
};

/// Pins: Query Entities on broker A by type with `entityMap` true and `NGSILD-EntityMap` naming a map broker A
/// created over two Entities answers 200 with those two only and the same `NGSILD-EntityMap`, although a third
/// Entity was created after the map
/// Clause: 5.7.2.4, 5.5.14, 6.3.15, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_7608_a_named_map_is_reused_and_named_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 2)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
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
    let map_header = page.entity_map().valid().or_fail()?;
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 3)
        .build()?;
    broker.entities().create(&vehicle_3).send().await?.done()?;
    let entity_map_request_2 = EntityMapRequest::returned().reusing(map_header.clone());
    let page_2 = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request_2)
        .send()
        .await?
        .expecting(StatusCode::OK)?;
    expect_that!(page_2.entity_map().valid(), some(eq(map_header)));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), not(contains(has_id(&vehicle_3_id))));
    Ok(())
}
