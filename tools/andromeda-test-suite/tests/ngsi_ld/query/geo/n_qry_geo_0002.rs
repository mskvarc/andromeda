//! Equals with a polygon selects the topologically equal one.

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

/// Pins: Georel=equals with a Polygon selects the entity whose Polygon is topologically equal, whichever vertex its
/// ring starts at
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel equals maps to Equals, a.Equals(b) iff a within b and b within a, a.Relate(b, "TFFFTFFFT"),
/// which is topological and so independent of the vertex a ring starts at
#[gtest]
#[tokio::test]
async fn n_qry_geo_0002_equals_with_a_polygon_selects_the_topologically_equal_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let block_id = namespace.urn("Block", 1)?;
    let block_type = namespace.entity_type("Block")?;
    let block_2_id = namespace.urn("Block", 2)?;
    let parcel_id = namespace.urn("Parcel", 1)?;
    let parcel_type = namespace.entity_type("Parcel")?;
    let parcel_2_id = namespace.urn("Parcel", 2)?;
    let plot_id = namespace.urn("Plot", 1)?;
    let plot_type = namespace.entity_type("Plot")?;
    let entities = [
        Entity::builder()
            .id(block_id.clone())
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
            .id(block_2_id)
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
            .id(parcel_id.clone())
            .entity_type(parcel_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(13.286_590_6, 52.564_864_5)?,
                    Position::try_new(13.287_963_9, 52.564_864_5)?,
                    Position::try_new(13.279_724_1, 52.498_867_9)?,
                    Position::try_new(13.477_478, 52.471_270_3)?,
                    Position::try_new(13.504_943_8, 52.537_308_4)?,
                    Position::try_new(13.286_590_6, 52.564_864_5)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(parcel_2_id)
            .entity_type(parcel_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(13.3, 52.48)?,
                    Position::try_new(13.38, 52.48)?,
                    Position::try_new(13.38, 52.52)?,
                    Position::try_new(13.3, 52.52)?,
                    Position::try_new(13.3, 52.48)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(plot_id.clone())
            .entity_type(plot_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(0.0, 0.0)?,
                    Position::try_new(1.0, 0.0)?,
                    Position::try_new(1.0, 1.0)?,
                    Position::try_new(0.0, 1.0)?,
                    Position::try_new(0.0, 0.0)?,
                ])?])?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.287_963_9, 52.564_864_5)?,
            Position::try_new(13.286_590_6, 52.564_864_5)?,
            Position::try_new(13.279_724_1, 52.498_867_9)?,
            Position::try_new(13.477_478, 52.471_270_3)?,
            Position::try_new(13.504_943_8, 52.537_308_4)?,
            Position::try_new(13.287_963_9, 52.564_864_5)?,
        ])?])?)))
        .entity_type(&block_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&block_id)));
    // the same ring as the stored one.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.286_590_6, 52.564_864_5)?,
            Position::try_new(13.287_963_9, 52.564_864_5)?,
            Position::try_new(13.279_724_1, 52.498_867_9)?,
            Position::try_new(13.477_478, 52.471_270_3)?,
            Position::try_new(13.504_943_8, 52.537_308_4)?,
            Position::try_new(13.286_590_6, 52.564_864_5)?,
        ])?])?)))
        .entity_type(&parcel_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&parcel_id)));
    // The same ring starting at another vertex.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
        ])?])?)))
        .entity_type(&plot_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&plot_id)));
    Ok(())
}
