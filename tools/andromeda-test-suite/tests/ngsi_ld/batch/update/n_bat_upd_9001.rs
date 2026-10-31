//! No overwrite adds the fragment types.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{listing, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Update with `options=noOverwrite` of a fragment typed Car on a Vehicle answers 204, and the
/// entity reads back with both types and the appended Attribute
/// Clause: 5.6.9.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_bat_upd_9001_no_overwrite_adds_the_fragment_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 80)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), car_type.clone())
        .property("brand", "Mercedes")
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(&entity.entity_type, listing_count(2));
    expect_that!(&entity.entity_type, listing(&vehicle_type));
    expect_that!(&entity.entity_type, listing(&car_type));
    expect_that!(entity, has_attribute("brand", any_attribute().value("Mercedes")));
    Ok(())
}
