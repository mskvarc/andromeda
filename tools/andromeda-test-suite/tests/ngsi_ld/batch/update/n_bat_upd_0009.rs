//! A type list adds the new type.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{listing, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: a Batch Entity Update whose fragment names the entity's type and a second type answers 204, and the entity
/// reads back with both types
/// Clause: 5.6.9.4, 5.6.3.4, 4.16, 6.16.3.1
#[gtest]
#[tokio::test]
async fn n_bat_upd_0009_a_type_list_adds_the_new_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_id.clone(), vec![building_type.clone(), namespace.entity_type("Vehicle")?]).build()?];
    broker.entities().batch_update(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(&entity.entity_type, listing_count(2));
    expect_that!(&entity.entity_type, listing(&building_type));
    expect_that!(&entity.entity_type, listing(&vehicle_type));
    Ok(())
}
