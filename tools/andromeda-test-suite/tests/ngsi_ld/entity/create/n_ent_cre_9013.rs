//! The axis ends and any altitude are stored as sent.

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

/// Pins: Create Entity with a location at longitude 180 and latitude 90, an `observationSpace` at -180 and -90 and
/// an `operationSpace` at altitude -11000 answers 201, and each position reads back as sent
/// Clause: 4.7, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9013_the_axis_ends_and_any_altitude_are_stored_as_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let range_probe_axis_ends_id = namespace.urn("RangeProbe", "axis-ends")?;
    let range_probe_type = namespace.entity_type("RangeProbe")?;
    let range_probe_axis_ends = Entity::builder()
        .id(range_probe_axis_ends_id.clone())
        .entity_type(range_probe_type)
        .geo_property("location", Shape::Point(Position::try_new(180.0, 90.0)?))
        .geo_property("observationSpace", Shape::Point(Position::try_new(-180.0, -90.0)?))
        .geo_property("operationSpace", Shape::Point(Position::try_with_altitude(14.5, 46.05, -11000.0)?))
        .build()?;
    broker.entities().create(&range_probe_axis_ends).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_axis_ends_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute().value_member(["coordinates", "0"], 180).value_member(["coordinates", "1"], 90)
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "observationSpace",
            any_attribute().value_member(["coordinates", "0"], -180).value_member(["coordinates", "1"], -90)
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "operationSpace",
            any_attribute()
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("14.5".parse()?))
                .value_member(["coordinates", "1"], Value::Number("46.05".parse()?))
                .value_member(["coordinates", "2"], -11000)
        )
    );
    Ok(())
}
