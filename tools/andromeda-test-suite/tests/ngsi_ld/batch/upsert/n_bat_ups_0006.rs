//! Replacing a relationship with a property carrying one keeps only the new content.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, relationship},
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Upsert without options of an entity holding a Relationship, with an item carrying a
/// Property with a Relationship sub-attribute, answers 204 and the entity holds the new Property and its
/// sub-attribute and no longer the old Relationship
/// Clause: 5.6.8.4, 5.6.8.5, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0006_replacing_a_relationship_with_a_property_carrying_one_keeps_only_the_new_content() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .relationship("locatedAt", city_paris_id)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("locatedAt"));
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            any_attribute().sub_attribute("hasWeatherObserved", relationship().object(sensor_id.clone()))
        )
    );
    Ok(())
}
