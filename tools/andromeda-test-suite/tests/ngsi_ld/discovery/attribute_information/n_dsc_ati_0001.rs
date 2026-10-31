//! Counts one instance per entity carrying it.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, Property},
};

/// Pins: Retrieve Available Attribute Information counts the attribute once per entity carrying it: two entities
/// holding it answer `attributeCount` 2
/// Clause: 5.7.10.4, 5.2.28, 6.28.3.1
#[gtest]
#[tokio::test]
async fn n_dsc_ati_0001_counts_one_instance_per_entity_carrying_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_type = namespace.entity_type("Building")?;
    let air_quality_level = namespace.attribute("airQualityLevel")?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property(
                namespace.attribute("airQualityLevel")?,
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?),
            )
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property(
                namespace.attribute("airQualityLevel")?,
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).attributes().retrieve(&air_quality_level).send().await?;
    expect_that!(information.attribute_name, eq(&air_quality_level));
    expect_that!(information.attribute_count, some(eq(2)));
    expect_that!(information.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(information.type_names.as_deref(), some(elements_are![eq(&building_type)]));
    Ok(())
}
