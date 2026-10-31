//! Format normalized answers the normalized representation.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    core_context::core_entry,
    entity_matcher::{attribute_members, has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Context, Entity, Lossless, NgsiLdMediaType, OneOrMany, Property},
};

/// Pins: Retrieve Entity with format=normalized answers the normalized representation: each attribute with its
/// type, value and metadata members
/// Clause: 6.3.7, 4.5.4, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0007_format_normalized_answers_the_normalized_representation() -> Result<()> {
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
        .retrieve(&building_id)
        .form(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().explicit_normalized())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(member_count(entity), eq(7));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(attribute_members(entity, "name"), some(eq(2)));
    expect_that!(attribute_members(entity, "airQualityLevel"), some(eq(4)));
    expect_that!(attribute_members(entity, "almostFull"), some(eq(2)));
    expect_that!(attribute_members(entity, "subCategory"), some(eq(2)));
    expect_that!(entity, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            property().value(4).observed_at("2020-09-09T16:40:00Z".parse()?).unit_code(UnitCode::C62)
        )
    );
    expect_that!(entity, has_attribute("almostFull", property().value(false)));
    expect_that!(entity, has_attribute("subCategory", property().value("tourism")));
    Ok(())
}
