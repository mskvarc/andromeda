//! Near with both distances selects the band between them.

use andromeda_test_suite::{
    broker,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    raw_call::{call, target},
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{
    Entity,
    Input,
    geometry::{Position, Shape},
};

/// Pins: Near;minDistance==500;maxDistance==10000 selects only the entities between the two distances
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the 4.10 `nearRel` grammar admits one distance modifier, so a
/// `minDistance` and a `maxDistance` together are outside it; Andromeda reads the pair as a band. The extension has a
/// configuration switch, and the test holds only while the switch is on; with it off the answer is 400 `BadRequestData`
#[gtest]
#[tokio::test]
async fn a_qry_geo_0011_near_with_both_distances_selects_the_band_between_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_type = namespace.entity_type("Spot")?;
    let spots = [("close", 13.38983), ("in-band", 13.433), ("far", 14.0)]
        .into_iter()
        .map(|(name, longitude)| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Spot", name)?)
                .entity_type(spot_type.clone())
                .geo_property("location", Shape::Point(Position::try_new(longitude, 52.534_473)?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&spots).send().await?.done()?;
    // About 60 m, 3 km and 41 km away from the reference point. The geoquery model writes one distance of `near`, so
    // the query is sent as written.
    let spot_type_text = spot_type.to_string();
    let answer = call(
        &broker,
        Method::GET,
        &target(
            "entities",
            &[
                ("type", spot_type_text.as_str()),
                ("georel", "near;minDistance==500;maxDistance==10000"),
                ("geometry", "Point"),
                ("coordinates", "[13.38893,52.534473]"),
            ],
        ),
        &[("Accept", "application/json")],
        None,
    )
    .await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(entities(&answer)?.as_slice(), ids_in_order(&[namespace.urn("Spot", "in-band")?]));
    Ok(())
}
