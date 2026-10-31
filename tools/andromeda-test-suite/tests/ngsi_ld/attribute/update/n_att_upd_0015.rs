//! A type added to a multi typed entity joins the others.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{listing, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Value},
};

/// Pins: Update Attributes with a new type on an Entity that already carries two types answers 204 and the Entity
/// carries all three
/// Clause: 5.6.2.4, 4.16
#[gtest]
#[tokio::test]
async fn n_att_upd_0015_a_type_added_to_a_multi_typed_entity_joins_the_others() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .entity_type(vec![vehicle_type.clone(), namespace.entity_type("Car")?])
        .build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().entity_type(building_type.clone()).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(&entity.entity_type, listing_count(3));
    expect_that!(&entity.entity_type, listing(&vehicle_type));
    expect_that!(&entity.entity_type, listing(&car_type));
    expect_that!(&entity.entity_type, listing(&building_type));
    Ok(())
}
