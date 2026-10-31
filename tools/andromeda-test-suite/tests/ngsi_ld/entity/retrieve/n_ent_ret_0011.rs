//! Normalized renders observation space as a geo property.

use andromeda_test_suite::{
    attribute_matcher::geo_property,
    broker,
    core_context::core_entry,
    entity_matcher::{attribute_members, has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Context,
        Entity,
        NgsiLdMediaType,
        OneOrMany,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity renders the `observationSpace` `GeoProperty` as a `GeoProperty` carrying its geometry in
/// the normalized representation, as JSON and as JSON-LD
/// Clause: 4.7, 4.5.4, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0011_normalized_renders_observation_space_as_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("observationSpace", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(3));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(attribute_members(entity, "observationSpace"), some(eq(2)));
    expect_that!(
        entity,
        has_attribute(
            "observationSpace",
            geo_property()
                .length_at([], 2)
                .value_member(["type"], "Point")
                .length_at(["coordinates"], 2)
                .value_member(["coordinates", "0"], Value::Number("13.3986".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.5547".parse()?))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(member_count(entity_2), eq(4));
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity_2.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(attribute_members(entity_2, "observationSpace"), some(eq(2)));
    expect_that!(
        entity_2,
        has_attribute(
            "observationSpace",
            geo_property()
                .length_at([], 2)
                .value_member(["type"], "Point")
                .length_at(["coordinates"], 2)
                .value_member(["coordinates", "0"], Value::Number("13.3986".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.5547".parse()?))
        )
    );
    Ok(())
}
