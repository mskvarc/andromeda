//! An ordinate whose nearest double is an axis end is that end.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityId,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: A Point written as `[180.00000000000000001, -90.000000000000000001]` is created and reads back as
/// `[180,-90]`
/// Clause: 4.7.2, 5.6.1
/// Recorded: clause 4.7.2 leaves the precision of a position to the implementation; Andromeda holds every ordinate
/// as the double nearest to it and checks the IETF RFC 7946 section 4 ranges on that double, so an ordinate whose
/// nearest double is an axis end is that end
#[gtest]
#[tokio::test]
async fn o_ent_cre_9050_an_ordinate_whose_nearest_double_is_an_axis_end_is_that_end() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let station_type = namespace.entity_type("Station")?;
    let ngsi_ld_station_id = format!("urn:ngsi-ld:Station:{namespace}").parse::<EntityId>()?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Station:{namespace}").parse::<EntityId>()?)
        .entity_type(station_type)
        .geo_property(
            "location",
            Shape::Point(Position::try_from_numbers("180.00000000000000001".parse()?, "-90.000000000000000001".parse()?)?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&entity).send().await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_station_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"coordinates\":[180,-90]"));
    Ok(())
}
