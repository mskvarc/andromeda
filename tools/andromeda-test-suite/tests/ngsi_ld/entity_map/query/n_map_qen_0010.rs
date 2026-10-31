//! Fifty matching entities are all mapped.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType, Property},
};

/// Pins: Create `EntityMap` for Query Entities over fifty matching Entities maps all fifty, a page limit not
/// applying to the map
/// Clause: 5.14.4.3, 5.14.4.4
#[gtest]
#[tokio::test]
async fn n_map_qen_0010_fifty_matching_entities_are_all_mapped() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_0_id = namespace.urn("Vehicle", "0")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_49_id = namespace.urn("Vehicle", 49)?;
    let entities = (0..=9)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", n)?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .chain((10..=49).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:02}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(50)));
    expect_that!(map.entity_map.get(&vehicle_0_id), some(anything()));
    expect_that!(map.entity_map.get(&vehicle_49_id), some(anything()));
    Ok(())
}
