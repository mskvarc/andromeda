//! Format key values answers the simplified representation.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Context, Entity, NgsiLdMediaType, OneOrMany, Property},
};

/// Pins: Retrieve Entity with `format=keyValues` answers the simplified representation: each attribute by its bare
/// value
/// Clause: 6.3.7, 4.5.4, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0005_format_key_values_answers_the_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
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
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(simplified_member_count(entity), eq(7));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_value("name", "Eiffel Tower"));
    expect_that!(entity, has_value("airQualityLevel", 4));
    expect_that!(entity, has_value("almostFull", false));
    expect_that!(entity, has_value("subCategory", "tourism"));
    Ok(())
}
