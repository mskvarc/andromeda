//! A property with a relationship sub attribute is added.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Update adding Properties, one carrying a Relationship sub-attribute, to two entities that
/// hold no Attribute answers 204, and the entities read back with them
/// Clause: 5.6.9.4, 5.6.9.5, 5.6.3.4, 6.16.3.1
#[gtest]
#[tokio::test]
async fn n_bat_upd_0003_a_property_with_a_relationship_sub_attribute_is_added() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .property("name", "Eiffel Tower")
            .property(
                "airQualityLevel",
                Property::builder(4)
                    .unit_code(UnitCode::C62)
                    .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                    .relationship("hasWeatherObserved", sensor_id.clone()),
            )
            .build()?,
        EntityFragment::for_entity(building_2_id.clone(), building_type.clone())
            .property("name", "Pisa Tower")
            .property("airQualityLevel", Property::builder(5).relationship("hasWeatherObserved", sensor_2_id.clone()))
            .build()?,
    ];
    broker.entities().batch_update(&fragments).send().await?.done()?;
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
        has_attribute(
            "airQualityLevel",
            any_attribute()
                .value(4)
                .sub_attribute("hasWeatherObserved", any_attribute().object(sensor_id.clone()))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "airQualityLevel",
            any_attribute().sub_attribute("hasWeatherObserved", any_attribute().object(sensor_2_id.clone()))
        )
    );
    Ok(())
}
