//! A null sub attribute is deleted and nothing else.

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
    model::{Attribute, AttributeKind, AttributeName, Entity, InstanceDeletion, NgsiLdMediaType, PatchMember, Property},
};

/// Pins: Partial Attribute Update setting `observedAt`, `unitCode` or a sub-Relationship object to urn:ngsi-ld:null
/// answers 204 and removes that member alone, keeping the value and every other member
/// Clause: 5.5.8, 5.6.4.4, 4.5.0, 4.5.3
#[gtest]
#[tokio::test]
async fn n_att_pat_9012_a_null_sub_attribute_is_deleted_and_nothing_else() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_partial_update_observed_at_id = namespace.urn("Building", "partial-update-observedAt")?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_partial_update_unit_code_id = namespace.urn("Building", "partial-update-unitCode")?;
    let building_partial_update_has_weather_observed_id = namespace.urn("Building", "partial-update-hasWeatherObserved")?;
    let building_partial_update_observed_at = Entity::builder()
        .id(building_partial_update_observed_at_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_partial_update_observed_at).send().await?.done()?;
    let patch = Attribute::from(Property::builder(4).build()?)
        .into_patch()
        .delete_member(PatchMember::ObservedAt)?;
    broker
        .entities()
        .update_attribute(&building_partial_update_observed_at_id, &"airQualityLevel".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_update_observed_at_id)
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
    let building_partial_update_unit_code = Entity::builder()
        .id(building_partial_update_unit_code_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_partial_update_unit_code).send().await?.done()?;
    let patch_2 = Attribute::from(Property::builder(4).build()?)
        .into_patch()
        .delete_member(PatchMember::UnitCode)?;
    broker
        .entities()
        .update_attribute(&building_partial_update_unit_code_id, &"airQualityLevel".parse::<AttributeName>()?, patch_2)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_update_unit_code_id)
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
    let building_partial_update_has_weather_observed = Entity::builder()
        .id(building_partial_update_has_weather_observed_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id.clone()),
        )
        .build()?;
    broker.entities().create(&building_partial_update_has_weather_observed).send().await?.done()?;
    let patch_3 = Attribute::from(Property::builder(4).build()?)
        .into_patch()
        .delete_sub_attribute("hasWeatherObserved", InstanceDeletion::new(AttributeKind::Relationship))?;
    broker
        .entities()
        .update_attribute(
            &building_partial_update_has_weather_observed_id,
            &"airQualityLevel".parse::<AttributeName>()?,
            patch_3,
        )
        .send()
        .await?
        .done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_update_has_weather_observed_id)
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
