//! Overlaps ignores the altitude of the polygons.

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

/// Pins: a three-dimensional Polygon overlaps a reference Polygon drawn at 900 m over part of its footprint, and a
/// Polygon elsewhere does not
/// Clause: 4.10, 4.7.1, 5.7.2
/// Recorded: clause 4.10: "the target geometry shall overlap, as specified by `[14]`, the reference geometry"; OGC
/// 06-103r4 clause 6.1.15.3: a.Overlaps(b) <=> (dim(I(a)) = dim(I(b)) = dim(I(a) intersect I(b))) and a intersect b
/// is neither a nor b; clause 4.10 defers the geometric relationships to OGC 06-103r4, whose clause 6.1.2.5 says
/// spatial operations work in the "map geometry" of the data and "will therefore not reflect z or m values in
/// calculations" and whose clause 6.1.15.1 defines the relationships by the projection of the objects onto the
/// horizontal surface, so a georel that ignores the altitude is the specified behaviour; the user ruled this on
/// 2026-10-06
#[gtest]
#[tokio::test]
async fn n_qry_geo_0077_overlaps_ignores_the_altitude_of_the_polygons() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let slab_covering_id = namespace.urn("Slab", "covering")?;
    let slab_type = namespace.entity_type("Slab")?;
    let slab_far_id = namespace.urn("Slab", "far")?;
    let entities = [
        Entity::builder()
            .id(slab_covering_id.clone())
            .entity_type(slab_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_with_altitude(13.0, 52.0, 0.0)?,
                    Position::try_with_altitude(14.0, 52.0, 0.0)?,
                    Position::try_with_altitude(14.0, 53.0, 0.0)?,
                    Position::try_with_altitude(13.0, 53.0, 0.0)?,
                    Position::try_with_altitude(13.0, 52.0, 0.0)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(slab_far_id)
            .entity_type(slab_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_with_altitude(30.0, 10.0, 0.0)?,
                    Position::try_with_altitude(31.0, 10.0, 0.0)?,
                    Position::try_with_altitude(31.0, 11.0, 0.0)?,
                    Position::try_with_altitude(30.0, 11.0, 0.0)?,
                    Position::try_with_altitude(30.0, 10.0, 0.0)?,
                ])?])?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::overlaps(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_with_altitude(13.5, 52.5, 900.0)?,
            Position::try_with_altitude(14.5, 52.5, 900.0)?,
            Position::try_with_altitude(14.5, 53.5, 900.0)?,
            Position::try_with_altitude(13.5, 53.5, 900.0)?,
            Position::try_with_altitude(13.5, 52.5, 900.0)?,
        ])?])?)))
        .entity_type(&slab_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&slab_covering_id)));
    Ok(())
}
