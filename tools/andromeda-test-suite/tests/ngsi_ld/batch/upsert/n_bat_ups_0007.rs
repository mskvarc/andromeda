//! Replacing drops a sub attribute the item omits.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Upsert without options of an entity whose Property carries a Relationship sub-attribute,
/// with an item carrying simple Properties only, answers 204 and the entity holds the new Properties without the
/// old sub-attribute
/// Clause: 5.6.8.4, 5.6.8.5, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0007_replacing_drops_a_sub_attribute_the_item_omits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("airQualityLevel", Property::builder(4).relationship("hasWeatherObserved", sensor_id))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("airQualityLevel", 6)
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(
        entity,
        has_attribute("airQualityLevel", any_attribute().value(6).without_sub_attribute("hasWeatherObserved"))
    );
    Ok(())
}
