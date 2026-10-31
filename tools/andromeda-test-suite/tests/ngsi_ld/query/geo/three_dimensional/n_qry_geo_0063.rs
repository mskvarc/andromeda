//! A three dimensional multi polygon reads back with every ordinate.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: a three-dimensional `MultiPolygon` reads back with every ordinate of every position, altitude included
/// Clause: 4.7.1, 4.7.2, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0063_a_three_dimensional_multi_polygon_reads_back_with_every_ordinate() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            Shape::MultiPolygon(vec![
                Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_with_altitude(13.0, 52.0, 5.0)?,
                    Position::try_with_altitude(13.2, 52.0, 5.0)?,
                    Position::try_with_altitude(13.2, 52.2, 5.0)?,
                    Position::try_with_altitude(13.0, 52.2, 5.0)?,
                    Position::try_with_altitude(13.0, 52.0, 5.0)?,
                ])?])?,
                Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_with_altitude(14.0, 53.0, 6.0)?,
                    Position::try_with_altitude(14.2, 53.0, 6.0)?,
                    Position::try_with_altitude(14.2, 53.2, 6.0)?,
                    Position::try_with_altitude(14.0, 53.2, 6.0)?,
                    Position::try_with_altitude(14.0, 53.0, 6.0)?,
                ])?])?,
            ]),
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
                .value_member(["type"], "MultiPolygon")
                .length_at(["coordinates"], 2)
                .length_at(["coordinates", "0"], 1)
                .length_at(["coordinates", "0", "0"], 5)
                .length_at(["coordinates", "0", "0", "0"], 3)
                .value_member(["coordinates", "0", "0", "0", "0"], 13)
                .value_member(["coordinates", "0", "0", "0", "1"], 52)
                .value_member(["coordinates", "0", "0", "0", "2"], 5)
                .length_at(["coordinates", "0", "0", "1"], 3)
                .value_member(["coordinates", "0", "0", "1", "0"], Value::Number("13.2".parse()?))
                .value_member(["coordinates", "0", "0", "1", "1"], 52)
                .value_member(["coordinates", "0", "0", "1", "2"], 5)
                .length_at(["coordinates", "0", "0", "2"], 3)
                .value_member(["coordinates", "0", "0", "2", "0"], Value::Number("13.2".parse()?))
                .value_member(["coordinates", "0", "0", "2", "1"], Value::Number("52.2".parse()?))
                .value_member(["coordinates", "0", "0", "2", "2"], 5)
                .length_at(["coordinates", "0", "0", "3"], 3)
                .value_member(["coordinates", "0", "0", "3", "0"], 13)
                .value_member(["coordinates", "0", "0", "3", "1"], Value::Number("52.2".parse()?))
                .value_member(["coordinates", "0", "0", "3", "2"], 5)
                .length_at(["coordinates", "0", "0", "4"], 3)
                .value_member(["coordinates", "0", "0", "4", "0"], 13)
                .value_member(["coordinates", "0", "0", "4", "1"], 52)
                .value_member(["coordinates", "0", "0", "4", "2"], 5)
                .length_at(["coordinates", "1"], 1)
                .length_at(["coordinates", "1", "0"], 5)
                .length_at(["coordinates", "1", "0", "0"], 3)
                .value_member(["coordinates", "1", "0", "0", "0"], 14)
                .value_member(["coordinates", "1", "0", "0", "1"], 53)
                .value_member(["coordinates", "1", "0", "0", "2"], 6)
                .length_at(["coordinates", "1", "0", "1"], 3)
                .value_member(["coordinates", "1", "0", "1", "0"], Value::Number("14.2".parse()?))
                .value_member(["coordinates", "1", "0", "1", "1"], 53)
                .value_member(["coordinates", "1", "0", "1", "2"], 6)
                .length_at(["coordinates", "1", "0", "2"], 3)
                .value_member(["coordinates", "1", "0", "2", "0"], Value::Number("14.2".parse()?))
                .value_member(["coordinates", "1", "0", "2", "1"], Value::Number("53.2".parse()?))
                .value_member(["coordinates", "1", "0", "2", "2"], 6)
                .length_at(["coordinates", "1", "0", "3"], 3)
                .value_member(["coordinates", "1", "0", "3", "0"], 14)
                .value_member(["coordinates", "1", "0", "3", "1"], Value::Number("53.2".parse()?))
                .value_member(["coordinates", "1", "0", "3", "2"], 6)
                .length_at(["coordinates", "1", "0", "4"], 3)
                .value_member(["coordinates", "1", "0", "4", "0"], 14)
                .value_member(["coordinates", "1", "0", "4", "1"], 53)
                .value_member(["coordinates", "1", "0", "4", "2"], 6)
        )
    );
    Ok(())
}
