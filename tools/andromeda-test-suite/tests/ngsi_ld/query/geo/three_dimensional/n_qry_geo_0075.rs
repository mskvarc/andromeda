//! Disjoint ignores the altitude of the point.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: a Point 5000 m above the reference Polygon is not disjoint from it, and a Point beyond the footprint is
/// Clause: 4.10, 4.7.1, 5.7.2
/// Recorded: clause 4.10: "the target geometry shall be disjoint, as specified by `[14]`, to the reference
/// geometry"; OGC 06-103r4 clause 6.1.15.3: a.Disjoint(b) <=> a intersect b = empty; clause 4.10 defers the
/// geometric relationships to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations work in the "map geometry"
/// of the data and "will therefore not reflect z or m values in calculations" and whose clause 6.1.15.1 defines the
/// relationships by the projection of the objects onto the horizontal surface, so a georel that ignores the
/// altitude is the specified behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_qry_geo_0075_disjoint_ignores_the_altitude_of_the_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let beacon_over_id = namespace.urn("Beacon", "over")?;
    let beacon_type = namespace.entity_type("Beacon")?;
    let beacon_far_id = namespace.urn("Beacon", "far")?;
    let entities = [
        Entity::builder()
            .id(beacon_over_id)
            .entity_type(beacon_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 5000.0)?))
            .build()?,
        Entity::builder()
            .id(beacon_far_id.clone())
            .entity_type(beacon_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(30.0, 10.0, 5000.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::disjoint(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_with_altitude(13.0, 52.0, 0.0)?,
            Position::try_with_altitude(14.0, 52.0, 0.0)?,
            Position::try_with_altitude(14.0, 53.0, 0.0)?,
            Position::try_with_altitude(13.0, 53.0, 0.0)?,
            Position::try_with_altitude(13.0, 52.0, 0.0)?,
        ])?])?)))
        .entity_type(&beacon_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&beacon_far_id)));
    Ok(())
}
