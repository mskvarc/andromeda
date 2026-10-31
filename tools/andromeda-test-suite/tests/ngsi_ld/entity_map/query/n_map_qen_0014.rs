//! A joined query maps the linked entities it reached.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Property, Relationship},
    query::Join,
};

/// Pins: Query Entities with `entityMap=true`, join=flat and `joinLevel=1` creates an `EntityMap` holding the
/// matched Entity and the Linked Entity its Relationship reached
/// Clause: 4.5.23, 5.5.14, 5.14.4.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_0014_a_joined_query_maps_the_linked_entities_it_reached() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_linked_id = namespace.urn("Vehicle", "linked")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_root_id = namespace.urn("Vehicle", "root")?;
    let entities = [
        Entity::builder()
            .id(vehicle_linked_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(vehicle_root_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
            .relationship(
                "linkedTo",
                Relationship::builder(vehicle_linked_id.clone()).observed_at("2021-01-01T00:00:00Z".parse()?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![vehicle_root_id.clone()])
        .entity_type(&vehicle_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(map_2.entity_map.get(&vehicle_root_id), some(anything()));
    expect_that!(map_2.entity_map.get(&vehicle_linked_id), some(anything()));
    Ok(())
}
