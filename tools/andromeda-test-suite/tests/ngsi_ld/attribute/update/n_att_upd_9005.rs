//! A first level null removes the whole attribute.

use andromeda_test_suite::{broker, entity_matcher::lacks_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Update Attributes and Merge Entity setting an attribute to urn:ngsi-ld:null as a whole answer 204 and
/// remove the attribute
/// Clause: 5.5.8, 5.5.12, 5.6.2.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_att_upd_9005_a_first_level_null_removes_the_whole_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_update_attrs_first_level_id = namespace.urn("Building", "update-attrs-first-level")?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_merge_first_level_id = namespace.urn("Building", "merge-first-level")?;
    let building_update_attrs_first_level = Entity::builder()
        .id(building_update_attrs_first_level_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_update_attrs_first_level).send().await?.done()?;
    let fragment = EntityFragment::builder().delete("airQualityLevel").build()?;
    broker
        .entities()
        .update_attributes(&building_update_attrs_first_level_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_update_attrs_first_level_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("airQualityLevel"));
    expect_that!(entity.id, eq(&building_update_attrs_first_level_id));
    let building_merge_first_level = Entity::builder()
        .id(building_merge_first_level_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_merge_first_level).send().await?.done()?;
    let fragment_2 = EntityFragment::for_entity(building_merge_first_level_id.clone(), building_type.clone())
        .delete("airQualityLevel")
        .build()?;
    broker.entities().merge(&building_merge_first_level_id, &fragment_2).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_first_level_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("airQualityLevel"));
    expect_that!(entity_2.id, eq(&building_merge_first_level_id));
    Ok(())
}
