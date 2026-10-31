//! An ordinate reads back in the shortest spelling of its double.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: A Point written as `[179.8451356, -16.6429634]` reads back byte for byte, one written as
/// `[14.550000000000000001, 52.50]` reads back as `[14.55,52.5]`, and an altitude of 667.123456789012345678 reads
/// back as 667.1234567890124
/// Clause: 4.7.2, 5.7.1
/// Recorded: clause 4.7.2 leaves the precision of a position to the implementation; Andromeda holds every ordinate
/// as the double nearest to it and writes it in the shortest spelling that reads back as that double
#[gtest]
#[tokio::test]
async fn o_ent_cre_9049_an_ordinate_reads_back_in_the_shortest_spelling_of_its_double() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let station_shortest_id = namespace.urn("Station", "shortest")?;
    let station_type = namespace.entity_type("Station")?;
    let station_respelled_id = namespace.urn("Station", "respelled")?;
    let station_altitude_id = namespace.urn("Station", "altitude")?;
    let station_shortest = Entity::builder()
        .id(station_shortest_id.clone())
        .entity_type(station_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(179.845_135_6, -16.642_963_4)?))
        .build()?;
    broker.entities().create(&station_shortest).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&station_shortest_id)
        .send()
        .await?;
    expect_that!(
        String::from_utf8_lossy(&received.body),
        contains_substring("\"coordinates\":[179.8451356,-16.6429634]")
    );
    let station_respelled = Entity::builder()
        .id(station_respelled_id.clone())
        .entity_type(station_type.clone())
        .geo_property(
            "location",
            Shape::Point(Position::try_from_numbers("14.550000000000000001".parse()?, "52.50".parse()?)?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&station_respelled).send().await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&station_respelled_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring("\"coordinates\":[14.55,52.5]"));
    let station_altitude = Entity::builder()
        .id(station_altitude_id.clone())
        .entity_type(station_type.clone())
        .geo_property(
            "location",
            Shape::Point(Position::try_from_numbers_with_altitude(
                "13.5".parse()?,
                "52.5".parse()?,
                "667.123456789012345678".parse()?,
            )?),
        )
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&station_altitude).send().await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let received_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&station_altitude_id)
        .send()
        .await?;
    expect_that!(
        String::from_utf8_lossy(&received_3.body),
        contains_substring("\"coordinates\":[13.5,52.5,667.1234567890124]")
    );
    Ok(())
}
