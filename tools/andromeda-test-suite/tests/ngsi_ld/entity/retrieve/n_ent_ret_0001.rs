//! Answers every attribute of every kind.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of, geo_property, property, relationship},
    broker,
    core_context::core_entry,
    entity_matcher::{attribute_members, has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        Context,
        Entity,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity without a projection answers the entity with every attribute it holds, of every attribute
/// kind, as JSON-LD with the core @context and as JSON
/// Clause: 5.7.1, 5.2.4, 6.3.6
#[gtest]
#[tokio::test]
async fn n_ent_ret_0001_answers_every_attribute_of_every_kind() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "paris")?;
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
        .relationship("locatedIn", city_paris_id.clone())
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Avenue Anatole France"), ("en", "Anatole France Avenue")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(10));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(attribute_members(entity, "name"), some(eq(2)));
    expect_that!(attribute_members(entity, "subCategory"), some(eq(2)));
    expect_that!(attribute_members(entity, "airQualityLevel"), some(eq(4)));
    expect_that!(attribute_members(entity, "almostFull"), some(eq(2)));
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            any_attribute().value(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?)
        )
    );
    expect_that!(entity, has_attribute("almostFull", any_attribute().value(false)));
    expect_that!(entity, has_attribute("locatedIn", relationship().object(city_paris_id.clone())));
    expect_that!(entity, has_attribute("location", geo_property().value_member(["type"], "Point")));
    expect_that!(
        entity,
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Avenue Anatole France")
                .language("en", "Anatole France Avenue")
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(member_count(entity_2), eq(9));
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity_2, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity_2, has_attribute("airQualityLevel", any_attribute().value(4)));
    expect_that!(entity_2, has_attribute("almostFull", any_attribute().value(false)));
    expect_that!(entity_2, has_attribute("locatedIn", relationship().object(city_paris_id.clone())));
    expect_that!(
        entity_2,
        has_attribute(
            "location",
            geo_property()
                .value_member(["coordinates", "0"], Value::Number("2.2945".parse()?))
                .value_member(["coordinates", "1"], Value::Number("48.8584".parse()?))
        )
    );
    expect_that!(
        entity_2,
        has_attribute("street", attribute_of(AttributeKind::LanguageProperty).language("fr", "Avenue Anatole France"))
    );
    Ok(())
}
