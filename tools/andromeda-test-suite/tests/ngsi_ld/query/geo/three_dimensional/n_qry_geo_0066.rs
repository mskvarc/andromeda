//! Batch create keeps the altitude of each three dimensional point.

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

/// Pins: Batch Entity Creation keeps the altitude of each three-dimensional Point, an altitude of zero included
/// Clause: 4.7.1, 5.6.7
#[gtest]
#[tokio::test]
async fn n_qry_geo_0066_batch_create_keeps_the_altitude_of_each_three_dimensional_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_ground_id = namespace.urn("Building", "ground")?;
    let building_type = namespace.entity_type("Building")?;
    let building_elevated_id = namespace.urn("Building", "elevated")?;
    let entities = [
        Entity::builder()
            .id(building_ground_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?))
            .build()?,
        Entity::builder()
            .id(building_elevated_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 500.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_ground_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .value_member(["type"], "Point")
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "2"], 0)
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_elevated_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "location",
            any_attribute()
                .value_member(["type"], "Point")
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "2"], 500)
        )
    );
    Ok(())
}
