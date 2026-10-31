//! A q filter narrows the map when entities are not split.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Create `EntityMap` for Query Entities with q speed>50 maps only the Entity whose speed is 100 and not the
/// one whose speed is 30
/// Clause: 5.14.4.4, 4.9, 6.4.3.2
/// Recorded: 5.14.4.4 applies q while building the map only when Entities are not split, and 6.4.3.2 leaves the
/// `splitEntities` default to the implementation; Andromeda defaults to complete Entities
#[gtest]
#[tokio::test]
async fn o_map_qen_0003_a_q_filter_narrows_the_map_when_entities_are_not_split() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(100).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(30).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map.entity_map.get(&vehicle_2_id), none());
    Ok(())
}
