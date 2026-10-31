//! Simple properties read back with their values.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Creation of two entities with simple Properties answers 201, and each entity reads back
/// with every Property value and `unitCode`
/// Clause: 5.6.7.4, 6.14.3.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_bat_cre_0002_simple_properties_read_back_with_their_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .build()?,
    ];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, len(eq(2)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity, has_attribute("airQualityLevel", any_attribute().value(4).unit_code(UnitCode::C62)));
    expect_that!(entity, has_attribute("almostFull", any_attribute().value(false)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Pisa Tower")));
    Ok(())
}
