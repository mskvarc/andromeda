//! A three dimensional polygon reads back with every ordinate.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: a three-dimensional Polygon reads back with every ordinate of every ring position, altitude included
/// Clause: 4.7.1, 4.7.2, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0060_a_three_dimensional_polygon_reads_back_with_every_ordinate() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_with_altitude(13.0, 52.0, 10.0)?,
                Position::try_with_altitude(14.0, 52.0, 20.0)?,
                Position::try_with_altitude(14.0, 53.0, 30.0)?,
                Position::try_with_altitude(13.0, 53.0, 40.0)?,
                Position::try_with_altitude(13.0, 52.0, 10.0)?,
            ])?])?),
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
                .value_member(["type"], "Polygon")
                .length_at(["coordinates"], 1)
                .length_at(["coordinates", "0"], 5)
                .length_at(["coordinates", "0", "0"], 3)
                .value_member(["coordinates", "0", "0", "0"], 13)
                .value_member(["coordinates", "0", "0", "1"], 52)
                .value_member(["coordinates", "0", "0", "2"], 10)
                .length_at(["coordinates", "0", "1"], 3)
                .value_member(["coordinates", "0", "1", "0"], 14)
                .value_member(["coordinates", "0", "1", "1"], 52)
                .value_member(["coordinates", "0", "1", "2"], 20)
                .length_at(["coordinates", "0", "2"], 3)
                .value_member(["coordinates", "0", "2", "0"], 14)
                .value_member(["coordinates", "0", "2", "1"], 53)
                .value_member(["coordinates", "0", "2", "2"], 30)
                .length_at(["coordinates", "0", "3"], 3)
                .value_member(["coordinates", "0", "3", "0"], 13)
                .value_member(["coordinates", "0", "3", "1"], 53)
                .value_member(["coordinates", "0", "3", "2"], 40)
                .length_at(["coordinates", "0", "4"], 3)
                .value_member(["coordinates", "0", "4", "0"], 13)
                .value_member(["coordinates", "0", "4", "1"], 52)
                .value_member(["coordinates", "0", "4", "2"], 10)
        )
    );
    Ok(())
}
