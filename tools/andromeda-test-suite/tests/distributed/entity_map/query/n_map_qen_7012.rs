//! With split entities an entity lacking the attrs attribute is still a candidate.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::EntitySplitting,
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Entities with `splitEntities` true and attrs speed maps both the local Entity
/// holding speed and the local Entity lacking it, the attribute list being a filter a split query leaves to the
/// merged result
/// Clause: 5.14.4.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_map_qen_7012_with_split_entities_an_entity_lacking_the_attrs_attribute_is_still_a_candidate() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 20)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("colour", "red")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .split_entities(EntitySplitting::Split)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map.entity_map.get(&vehicle_2_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
