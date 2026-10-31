//! Closed rings of either winding and a two position line are stored as sent.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LineString, LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: Create Entity with a Polygon holding a hole, a clockwise Polygon and a `LineString` of two
/// three-dimensional positions answers 201, and each geometry reads back with the coordinates sent
/// Clause: 4.7.2, 5.6.1, 5.6.6
#[gtest]
#[tokio::test]
async fn n_ent_cre_9015_closed_rings_of_either_winding_and_a_two_position_line_are_stored_as_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ring_probe_admissible_id = namespace.urn("RingProbe", "admissible")?;
    let ring_probe_type = namespace.entity_type("RingProbe")?;
    let ring_probe_admissible = Entity::builder()
        .id(ring_probe_admissible_id.clone())
        .entity_type(ring_probe_type)
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![
                LinearRing::try_new(vec![
                    Position::try_new(0.0, 0.0)?,
                    Position::try_new(4.0, 0.0)?,
                    Position::try_new(4.0, 4.0)?,
                    Position::try_new(0.0, 4.0)?,
                    Position::try_new(0.0, 0.0)?,
                ])?,
                LinearRing::try_new(vec![
                    Position::try_new(1.0, 1.0)?,
                    Position::try_new(1.0, 2.0)?,
                    Position::try_new(2.0, 2.0)?,
                    Position::try_new(2.0, 1.0)?,
                    Position::try_new(1.0, 1.0)?,
                ])?,
            ])?),
        )
        .geo_property(
            "observationSpace",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(0.0, 0.0)?,
                Position::try_new(0.0, 1.0)?,
                Position::try_new(1.0, 1.0)?,
                Position::try_new(1.0, 0.0)?,
                Position::try_new(0.0, 0.0)?,
            ])?])?),
        )
        .geo_property(
            "operationSpace",
            Shape::LineString(LineString::try_new(vec![
                Position::try_with_altitude(0.0, 0.0, 50.0)?,
                Position::try_with_altitude(1.0, 1.0, 50.0)?,
            ])?),
        )
        .build()?;
    broker.entities().create(&ring_probe_admissible).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ring_probe_admissible_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .length_at(["coordinates"], 2)
                .length_at(["coordinates", "0"], 5)
                .length_at(["coordinates", "0", "0"], 2)
                .value_member(["coordinates", "0", "0", "0"], 0)
                .value_member(["coordinates", "0", "0", "1"], 0)
                .length_at(["coordinates", "0", "1"], 2)
                .value_member(["coordinates", "0", "1", "0"], 4)
                .value_member(["coordinates", "0", "1", "1"], 0)
                .length_at(["coordinates", "0", "2"], 2)
                .value_member(["coordinates", "0", "2", "0"], 4)
                .value_member(["coordinates", "0", "2", "1"], 4)
                .length_at(["coordinates", "0", "3"], 2)
                .value_member(["coordinates", "0", "3", "0"], 0)
                .value_member(["coordinates", "0", "3", "1"], 4)
                .length_at(["coordinates", "0", "4"], 2)
                .value_member(["coordinates", "0", "4", "0"], 0)
                .value_member(["coordinates", "0", "4", "1"], 0)
                .length_at(["coordinates", "1"], 5)
                .length_at(["coordinates", "1", "0"], 2)
                .value_member(["coordinates", "1", "0", "0"], 1)
                .value_member(["coordinates", "1", "0", "1"], 1)
                .length_at(["coordinates", "1", "1"], 2)
                .value_member(["coordinates", "1", "1", "0"], 1)
                .value_member(["coordinates", "1", "1", "1"], 2)
                .length_at(["coordinates", "1", "2"], 2)
                .value_member(["coordinates", "1", "2", "0"], 2)
                .value_member(["coordinates", "1", "2", "1"], 2)
                .length_at(["coordinates", "1", "3"], 2)
                .value_member(["coordinates", "1", "3", "0"], 2)
                .value_member(["coordinates", "1", "3", "1"], 1)
                .length_at(["coordinates", "1", "4"], 2)
                .value_member(["coordinates", "1", "4", "0"], 1)
                .value_member(["coordinates", "1", "4", "1"], 1)
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "observationSpace",
            any_attribute()
                .length_at(["coordinates"], 1)
                .length_at(["coordinates", "0"], 5)
                .length_at(["coordinates", "0", "0"], 2)
                .value_member(["coordinates", "0", "0", "0"], 0)
                .value_member(["coordinates", "0", "0", "1"], 0)
                .length_at(["coordinates", "0", "1"], 2)
                .value_member(["coordinates", "0", "1", "0"], 0)
                .value_member(["coordinates", "0", "1", "1"], 1)
                .length_at(["coordinates", "0", "2"], 2)
                .value_member(["coordinates", "0", "2", "0"], 1)
                .value_member(["coordinates", "0", "2", "1"], 1)
                .length_at(["coordinates", "0", "3"], 2)
                .value_member(["coordinates", "0", "3", "0"], 1)
                .value_member(["coordinates", "0", "3", "1"], 0)
                .length_at(["coordinates", "0", "4"], 2)
                .value_member(["coordinates", "0", "4", "0"], 0)
                .value_member(["coordinates", "0", "4", "1"], 0)
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "operationSpace",
            any_attribute()
                .length_at(["coordinates"], 2)
                .length_at(["coordinates", "0"], 3)
                .value_member(["coordinates", "0", "0"], 0)
                .value_member(["coordinates", "0", "1"], 0)
                .value_member(["coordinates", "0", "2"], 50)
                .length_at(["coordinates", "1"], 3)
                .value_member(["coordinates", "1", "0"], 1)
                .value_member(["coordinates", "1", "1"], 1)
                .value_member(["coordinates", "1", "2"], 50)
        )
    );
    Ok(())
}
