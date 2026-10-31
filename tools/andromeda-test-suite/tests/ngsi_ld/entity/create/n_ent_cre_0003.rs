//! A relationship on a property reads back as its sub attribute.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Relationship given as a sub-attribute of a Property is created with it and reads back nested in that
/// Property with its type and object
/// Clause: 5.6.1, 4.5.3, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0003_a_relationship_on_a_property_reads_back_as_its_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_43a1af27_id = namespace.urn("Sensor", "43a1af27")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_43a1af27_id.clone()),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            property()
                .value(4)
                .sub_attribute("hasWeatherObserved", relationship().object(sensor_43a1af27_id.clone()))
        )
    );
    Ok(())
}
