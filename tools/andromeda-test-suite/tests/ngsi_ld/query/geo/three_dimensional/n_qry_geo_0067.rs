//! A two dimensional location beside a three dimensional geo property keeps each arity.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: an entity with a two-ordinate location and a three-ordinate `observationPoint` reads back each position
/// with the arity it was written with
/// Clause: 4.7.1, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0067_a_two_dimensional_location_beside_a_three_dimensional_geo_property_keeps_each_arity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .geo_property("observationPoint", Shape::Point(Position::try_with_altitude(13.4, 52.52, 120.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .length_at(["coordinates"], 2)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "observationPoint",
            any_attribute()
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "2"], 120)
        )
    );
    Ok(())
}
