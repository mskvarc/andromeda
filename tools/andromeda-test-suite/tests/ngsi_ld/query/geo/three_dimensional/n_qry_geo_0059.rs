//! A three dimensional line string reads back with every ordinate.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{LineString, Position, Shape},
    },
};

/// Pins: a three-dimensional `LineString` reads back with every ordinate of every position, altitude included
/// Clause: 4.7.1, 4.7.2, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0059_a_three_dimensional_line_string_reads_back_with_every_ordinate() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![
                Position::try_with_altitude(13.4, 52.52, 100.0)?,
                Position::try_with_altitude(13.5, 52.53, 200.0)?,
                Position::try_with_altitude(13.6, 52.54, 300.0)?,
            ])?),
        )
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
                .value_member(["type"], "LineString")
                .length_at(["coordinates"], 3)
                .length_at(["coordinates", "0"], 3)
                .value_member(["coordinates", "0", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "0", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "0", "2"], 100)
                .length_at(["coordinates", "1"], 3)
                .value_member(["coordinates", "1", "0"], Value::Number("13.5".parse()?))
                .value_member(["coordinates", "1", "1"], Value::Number("52.53".parse()?))
                .value_member(["coordinates", "1", "2"], 200)
                .length_at(["coordinates", "2"], 3)
                .value_member(["coordinates", "2", "0"], Value::Number("13.6".parse()?))
                .value_member(["coordinates", "2", "1"], Value::Number("52.54".parse()?))
                .value_member(["coordinates", "2", "2"], 300)
        )
    );
    Ok(())
}
