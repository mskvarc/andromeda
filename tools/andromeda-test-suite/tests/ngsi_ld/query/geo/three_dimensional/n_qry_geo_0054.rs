//! A three dimensional point reads back simplified with its altitude.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_length_at, has_value_at},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: a three-dimensional Point location reads back in the simplified representation, selected by format and by
/// the options parameter, as the bare geometry with all three ordinates
/// Clause: 4.5.4, 4.7.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0054_a_three_dimensional_point_reads_back_simplified_with_its_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_value_at("location", ["type"], "Point"));
    expect_that!(entity, has_length_at("location", ["coordinates"], 3));
    expect_that!(entity, has_value_at("location", ["coordinates", "0"], Value::Number("13.4".parse()?)));
    expect_that!(entity, has_value_at("location", ["coordinates", "1"], Value::Number("52.52".parse()?)));
    expect_that!(entity, has_value_at("location", ["coordinates", "2"], 300));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value_at("location", ["type"], "Point"));
    expect_that!(entity_2, has_length_at("location", ["coordinates"], 3));
    expect_that!(entity_2, has_value_at("location", ["coordinates", "0"], Value::Number("13.4".parse()?)));
    expect_that!(entity_2, has_value_at("location", ["coordinates", "1"], Value::Number("52.52".parse()?)));
    expect_that!(entity_2, has_value_at("location", ["coordinates", "2"], 300));
    Ok(())
}
