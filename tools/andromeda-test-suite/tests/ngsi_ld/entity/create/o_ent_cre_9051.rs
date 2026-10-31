//! An ordinate reads back in the spelling the stored entity holds.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityId,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: A Mast whose location is the Point `[13, 52, 0.0000001]` and whose footprint Property value is the Point
/// `[13.0, 52.0]` reads back both as `[13,52,1E-7]` and `[13,52]` in the normalized, simplified and `GeoJSON`
/// representations, never as 13.0 or 1e-7
/// Clause: 4.5.2.2, 4.7.2, 5.7.1, 5.7.2
/// Recorded: clause 4.7.2 leaves the precision and spelling of a position to the implementation; Andromeda writes
/// every ordinate as the shortest decimal of its double, spelled the way it spells every number and the way the
/// stored entity holds it
#[gtest]
#[tokio::test]
async fn o_ent_cre_9051_an_ordinate_reads_back_in_the_spelling_the_stored_entity_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let mast_type = namespace.entity_type("Mast")?;
    let ngsi_ld_mast_id = format!("urn:ngsi-ld:Mast:{namespace}").parse::<EntityId>()?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Mast:{namespace}").parse::<EntityId>()?)
        .entity_type(mast_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.0, 52.0, 0.000_000_1)?))
        .property(
            "footprint",
            object([
                ("type", Value::from("Point")),
                (
                    "coordinates",
                    Value::Array(vec![Value::Number("13.0".parse()?), Value::Number("52.0".parse()?)]),
                ),
            ]),
        )
        .build()?;
    broker.entities().create(&entity).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_mast_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"coordinates\":[13,52,1E-7]"));
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"coordinates\":[13,52]"));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("13.0")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("1e-7")));
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&ngsi_ld_mast_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring("\"coordinates\":[13,52,1E-7]"));
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring("\"coordinates\":[13,52]"));
    expect_that!(String::from_utf8_lossy(&received_2.body), not(contains_substring("13.0")));
    expect_that!(String::from_utf8_lossy(&received_2.body), not(contains_substring("1e-7")));
    let received_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&ngsi_ld_mast_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_3.body), contains_substring("\"coordinates\":[13,52,1E-7]"));
    expect_that!(String::from_utf8_lossy(&received_3.body), contains_substring("\"coordinates\":[13,52]"));
    expect_that!(String::from_utf8_lossy(&received_3.body), not(contains_substring("13.0")));
    expect_that!(String::from_utf8_lossy(&received_3.body), not(contains_substring("1e-7")));
    Ok(())
}
