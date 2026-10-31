//! An entity no longer matching the filter leaves the map.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by type and q speed>50 with `entityMap` true maps only the Entity of speed 60, and the same
/// query naming the map answers it until its speed drops to 40, then answers none
/// Clause: 4.3.6.7, 5.5.14
#[gtest]
#[tokio::test]
async fn n_map_qen_6402_an_entity_no_longer_matching_the_filter_leaves_the_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(map_2.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map_2.entity_map.get(&vehicle_2_id), none());
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_id)));
    let fragment = EntityFragment::builder().property("speed", 40).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let entity_map_request_3 = EntityMapRequest::reuse(map.clone());
    let page_3 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .entity_map(&entity_map_request_3)
        .send()
        .await?;
    expect_that!(page_3.items(), is_empty());
    Ok(())
}
