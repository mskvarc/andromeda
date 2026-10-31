//! No overwrite adding only a sub attribute answers 207.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Update with `options=noOverwrite` whose fragment redeclares every Attribute the entity
/// holds and adds only a sub-attribute inside one of them appends no Attribute, answers 207 and leaves the entity
/// as it was
/// Clause: 5.6.9.4, 5.6.9.5, 5.6.3.4, 6.16.3.1
/// Recorded: clause 5.6.9.4 adds an entity to S when it "was successfully updated (Attributes appended)" and does
/// not say where an entity with nothing appended goes; a `noOverwrite` append compares top-level Attributes only,
/// so the new sub-attribute appends nothing, and Andromeda answers 207
#[gtest]
#[tokio::test]
async fn o_bat_upd_0006_no_overwrite_adding_only_a_sub_attribute_answers_207() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("airQualityLevel", Property::builder(4).unit_code(UnitCode::C62))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .property("name", "Eiffel Tower")
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).relationship("hasWeatherObserved", sensor_id),
        )
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.partial()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("airQualityLevel", any_attribute().value(4).without_sub_attribute("hasWeatherObserved"))
    );
    Ok(())
}
