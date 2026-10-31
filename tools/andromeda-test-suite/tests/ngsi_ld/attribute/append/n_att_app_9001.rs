//! The fragment types the entity lacks are added.

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
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Append Entity Attributes with a fragment typed `[Vehicle, Car]` on a Vehicle answers 204, and the entity
/// reads back with both types, Vehicle once, and the appended Attribute
/// Clause: 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_att_app_9001_the_fragment_types_the_entity_lacks_are_added() -> Result<()> {
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
    let fragment = EntityFragment::builder()
        .entity_type(vec![vehicle_type.clone(), namespace.entity_type("Car")?])
        .property("brand", "Mercedes")
        .build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
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
