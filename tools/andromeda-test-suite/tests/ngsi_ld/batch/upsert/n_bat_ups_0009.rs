//! Replacing takes the item type list.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{listing, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert without options of existing entities whose items name two types answers 204, and
/// each entity reads back with exactly those two types
/// Clause: 5.6.8.4, 5.6.8.5, 4.16, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0009_replacing_takes_the_item_type_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let tourist_destination_type = namespace.entity_type("TouristDestination")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let entities_2 = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(vec![building_type.clone(), namespace.entity_type("TouristDestination")?])
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(vec![building_type.clone(), namespace.entity_type("Vehicle")?])
            .build()?,
    ];
    broker.entities().batch_upsert(&entities_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(&entity.entity_type, listing_count(2));
    expect_that!(&entity.entity_type, listing(&building_type));
    expect_that!(&entity.entity_type, listing(&tourist_destination_type));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(&entity_2.entity_type, listing_count(2));
    expect_that!(&entity_2.entity_type, listing(&building_type));
    expect_that!(&entity_2.entity_type, listing(&vehicle_type));
    Ok(())
}
