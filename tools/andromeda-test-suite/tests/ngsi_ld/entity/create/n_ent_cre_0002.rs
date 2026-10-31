//! Simple properties read back with their values and unit code.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: an entity created with string, number and boolean Properties, one with a `unitCode` and an `observedAt`,
/// reads back with each Property's type, value and `unitCode`
/// Clause: 5.6.1, 4.5.2, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0002_simple_properties_read_back_with_their_values_and_unit_code() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
        )
        .property("almostFull", false)
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
    expect_that!(entity, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", property().value("tourism")));
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            property().value(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?)
        )
    );
    expect_that!(entity, has_attribute("almostFull", property().value(false)));
    Ok(())
}
