//! A merge deletes one null sub attribute and nothing else.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, Entity, EntityFragment, InstanceDeletion, NgsiLdMediaType, PatchMember, Property},
};

/// Pins: Merge Entity setting `observedAt`, `unitCode` or a sub-Relationship object to urn:ngsi-ld:null answers 204
/// and removes that member alone, the sub-Relationship as a whole
/// Clause: 5.5.12, 5.6.17.4, 4.5.0, 4.5.3
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9006_a_merge_deletes_one_null_sub_attribute_and_nothing_else() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_observed_at_id = namespace.urn("Building", "merge-observedAt")?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_merge_unit_code_id = namespace.urn("Building", "merge-unitCode")?;
    let building_merge_has_weather_observed_id = namespace.urn("Building", "merge-hasWeatherObserved")?;
    let building_merge_observed_at = Entity::builder()
        .id(building_merge_observed_at_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_merge_observed_at).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_merge_observed_at_id.clone(), building_type.clone())
        .patch(
            "airQualityLevel",
            Attribute::from(Property::builder(4).build()?)
                .into_patch()
                .delete_member(PatchMember::ObservedAt)?,
        )
        .build()?;
    broker.entities().merge(&building_merge_observed_at_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_observed_at_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            property()
                .without(Member::ObservedAt)
                .value(4)
                .with_member(Member::UnitCode)
                .sub_attribute("hasWeatherObserved", any_attribute())
        )
    );
    let building_merge_unit_code = Entity::builder()
        .id(building_merge_unit_code_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_merge_unit_code).send().await?.done()?;
    let fragment_2 = EntityFragment::for_entity(building_merge_unit_code_id.clone(), building_type.clone())
        .patch(
            "airQualityLevel",
            Attribute::from(Property::builder(4).build()?)
                .into_patch()
                .delete_member(PatchMember::UnitCode)?,
        )
        .build()?;
    broker.entities().merge(&building_merge_unit_code_id, &fragment_2).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_unit_code_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "airQualityLevel",
            property()
                .without(Member::UnitCode)
                .value(4)
                .with_member(Member::ObservedAt)
                .sub_attribute("hasWeatherObserved", any_attribute())
        )
    );
    let building_merge_has_weather_observed = Entity::builder()
        .id(building_merge_has_weather_observed_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_merge_has_weather_observed).send().await?.done()?;
    let fragment_3 = EntityFragment::for_entity(building_merge_has_weather_observed_id.clone(), building_type.clone())
        .patch(
            "airQualityLevel",
            Attribute::from(Property::builder(4).build()?)
                .into_patch()
                .delete_sub_attribute("hasWeatherObserved", InstanceDeletion::new(AttributeKind::Relationship))?,
        )
        .build()?;
    broker
        .entities()
        .merge(&building_merge_has_weather_observed_id, &fragment_3)
        .send()
        .await?
        .done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_has_weather_observed_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute(
            "airQualityLevel",
            property()
                .without_sub_attribute("hasWeatherObserved")
                .value(4)
                .with_member(Member::UnitCode)
                .with_member(Member::ObservedAt)
        )
    );
    Ok(())
}
