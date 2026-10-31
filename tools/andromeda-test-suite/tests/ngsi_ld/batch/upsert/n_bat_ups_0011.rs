//! Update mode replaces a property with the item one and its sub attribute.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, relationship},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Upsert with options=update of an existing entity, with an item carrying a Property with a
/// Relationship sub-attribute, answers 204 and the entity holds that sub-attribute beside its other Properties
/// Clause: 5.6.8.4, 5.6.8.5, 5.6.2.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0011_update_mode_replaces_a_property_with_the_item_one_and_its_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("airQualityLevel", 4)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("airQualityLevel", Property::builder(5).relationship("hasWeatherObserved", sensor_id))
        .build()?];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("airQualityLevel", any_attribute().value(5).sub_attribute("hasWeatherObserved", relationship()))
    );
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
