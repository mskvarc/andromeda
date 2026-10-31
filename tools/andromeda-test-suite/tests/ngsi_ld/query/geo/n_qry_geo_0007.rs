//! Intersects selects the entities sharing a point with the reference.

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

/// Pins: Georel=intersects selects the entities whose `GeoProperty` shares at least one point with the reference
/// geometry, a point on its boundary included
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel intersects maps to Intersects, a.Intersects(b) iff not a.Disjoint(b), which holds for any
/// shared point, so a point on the boundary of the reference polygon intersects it
#[gtest]
#[tokio::test]
async fn n_qry_geo_0007_intersects_selects_the_entities_sharing_a_point_with_the_reference() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let block_id = namespace.urn("Block", 1)?;
    let block_type = namespace.entity_type("Block")?;
    let block_2_id = namespace.urn("Block", 2)?;
    let hall_id = namespace.urn("Hall", 1)?;
    let hall_type = namespace.entity_type("Hall")?;
    let pin_edge_id = namespace.urn("Pin", "edge")?;
    let pin_type = namespace.entity_type("Pin")?;
    let pin_out_id = namespace.urn("Pin", "out")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("name", "Berlin Brandenburg International Airport")
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .property("name", "Grunewald")
            .geo_property("location", Shape::Point(Position::try_new(13.215_983, 52.470_441)?))
            .build()?,
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
            .id(pin_edge_id.clone())
            .entity_type(pin_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(2.0, 1.0)?))
            .build()?,
        Entity::builder()
            .id(pin_out_id)
            .entity_type(pin_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(3.0, 3.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
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
    // 019_10_08.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    // 019_11_02.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
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
    expect_that!(page_3.total(), some(eq(1)));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&block_2_id)));
    // A point on the edge x=2 intersects.
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(2.0, 0.0)?,
            Position::try_new(2.0, 2.0)?,
            Position::try_new(0.0, 2.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&pin_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&pin_edge_id)));
    Ok(())
}
