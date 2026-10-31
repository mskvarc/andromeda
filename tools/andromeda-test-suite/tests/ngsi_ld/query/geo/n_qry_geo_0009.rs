//! Overlaps selects the polygons sharing part of their interior.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: Georel=overlaps selects geometries of the same dimension that share part of their interior with the
/// reference: a polygon fully inside the reference and a point inside it do not overlap
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel overlaps maps to Overlaps, defined only for same-dimension pairs (P/P, L/L, A/A) as
/// a.Relate(b, "T*T***T**") for A/A, which needs I(a) in E(b) and E(a) in I(b), so a polygon fully inside the
/// reference does not overlap it and a Point against a Polygon is outside the predicate
#[gtest]
#[tokio::test]
async fn n_qry_geo_0009_overlaps_selects_the_polygons_sharing_part_of_their_interior() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let block_id = namespace.urn("Block", 1)?;
    let block_type = namespace.entity_type("Block")?;
    let block_2_id = namespace.urn("Block", 2)?;
    let hall_id = namespace.urn("Hall", 1)?;
    let hall_type = namespace.entity_type("Hall")?;
    let patch_inside_id = namespace.urn("Patch", "inside")?;
    let patch_type = namespace.entity_type("Patch")?;
    let patch_point_id = namespace.urn("Patch", "point")?;
    let entities = [
        Entity::builder()
            .id(block_id)
            .entity_type(block_type.clone())
            .property("name", "Eiffel Tower")
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(13.287_963_9, 52.564_864_5)?,
                    Position::try_new(13.286_590_6, 52.564_864_5)?,
                    Position::try_new(13.279_724_1, 52.498_867_9)?,
                    Position::try_new(13.477_478, 52.471_270_3)?,
                    Position::try_new(13.504_943_8, 52.537_308_4)?,
                    Position::try_new(13.287_963_9, 52.564_864_5)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(block_2_id.clone())
            .entity_type(block_type.clone())
            .property("name", "Eiffel Tower")
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(13.315_773, 52.475_720_9)?,
                    Position::try_new(13.339_119, 52.444_342_1)?,
                    Position::try_new(13.389_244_1, 52.466_937_1)?,
                    Position::try_new(13.315_773, 52.475_720_9)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(hall_id.clone())
            .entity_type(hall_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(0.0, 0.0)?,
                    Position::try_new(2.0, 0.0)?,
                    Position::try_new(2.0, 2.0)?,
                    Position::try_new(0.0, 2.0)?,
                    Position::try_new(0.0, 0.0)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(patch_inside_id)
            .entity_type(patch_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(0.5, 0.5)?,
                    Position::try_new(1.0, 0.5)?,
                    Position::try_new(1.0, 1.0)?,
                    Position::try_new(0.5, 1.0)?,
                    Position::try_new(0.5, 0.5)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(patch_point_id)
            .entity_type(patch_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(1.0, 1.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::overlaps(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(1.0, 1.0)?,
            Position::try_new(3.0, 1.0)?,
            Position::try_new(3.0, 3.0)?,
            Position::try_new(1.0, 3.0)?,
            Position::try_new(1.0, 1.0)?,
        ])?])?)))
        .entity_type(&hall_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&hall_id)));
    // 019_11_07.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::overlaps(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.345_642_1, 52.479_484_8)?,
            Position::try_new(13.364_181_5, 52.442_877_3)?,
            Position::try_new(13.417_739_9, 52.473_002_3)?,
            Position::try_new(13.345_985_4, 52.479_903)?,
            Position::try_new(13.345_642_1, 52.479_484_8)?,
        ])?])?)))
        .entity_type(&block_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&block_2_id)));
    // A polygon fully within the reference, and a point, do not overlap it.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::overlaps(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(2.0, 0.0)?,
            Position::try_new(2.0, 2.0)?,
            Position::try_new(0.0, 2.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&patch_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
