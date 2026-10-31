//! A batch merge deletes a null unit code.

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
    model::{Attribute, Entity, EntityFragment, NgsiLdMediaType, PatchMember, Property},
};

/// Pins: Batch Entity Merge setting `unitCode` to urn:ngsi-ld:null answers 204 and removes the `unitCode` alone
/// Clause: 5.6.20.4, 5.5.12
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9007_a_batch_merge_deletes_a_null_unit_code() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_merge_unit_code_id = namespace.urn("Building", "batch-merge-unit-code")?;
    let building_type = namespace.entity_type("Building")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_batch_merge_unit_code = Entity::builder()
        .id(building_batch_merge_unit_code_id.clone())
        .entity_type(building_type.clone())
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", sensor_id),
        )
        .build()?;
    broker.entities().create(&building_batch_merge_unit_code).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_batch_merge_unit_code_id.clone(), building_type.clone())
        .patch(
            "airQualityLevel",
            Attribute::from(Property::builder(4).build()?)
                .into_patch()
                .delete_member(PatchMember::UnitCode)?,
        )
        .build()?];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_merge_unit_code_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            property()
                .without(Member::UnitCode)
                .value(4)
                .with_member(Member::ObservedAt)
                .sub_attribute("hasWeatherObserved", any_attribute())
        )
    );
    Ok(())
}
