//! Contains selects the entities whose geometry contains the reference.

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

/// Pins: Georel=contains selects the entities whose `GeoProperty` contains the reference geometry: a Polygon
/// containing a Point or a Polygon, a Point containing an equal Point, and never a Point containing a Polygon
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel contains maps to Contains, a.Contains(b) iff b.Within(a), so the stored geometry must reach
/// the interior of the reference: a Point contains only an equal Point and never a Polygon, whose interior lies in
/// E(Point)
#[gtest]
#[tokio::test]
async fn n_qry_geo_0006_contains_selects_the_entities_whose_geometry_contains_the_reference() -> Result<()> {
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
    let spot_id = namespace.urn("Spot", 1)?;
    let spot_type = namespace.entity_type("Spot")?;
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
            .id(spot_id)
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(1.0, 1.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::contains(Shape::Point(Position::try_new(1.0, 1.0)?)))
        .entity_type(&hall_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&hall_id)));
    // 019_10_07: a Point contains an equal Point.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::contains(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    // 019_11_03.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::contains(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.384_437_6, 52.535_696_6)?,
            Position::try_new(13.381_691, 52.512_301_2)?,
            Position::try_new(13.423_576_4, 52.510_002_7)?,
            Position::try_new(13.433_189_4, 52.527_342_5)?,
            Position::try_new(13.384_437_6, 52.535_696_6)?,
        ])?])?)))
        .entity_type(&block_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(1)));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&block_id)));
    // 019_11_04.
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::contains(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.136_901_9, 52.519_822_5)?,
            Position::try_new(13.136_558_5, 52.520_031_4)?,
            Position::try_new(13.131_752, 52.498_090_7)?,
            Position::try_new(13.183_250_4, 52.502_688_7)?,
            Position::try_new(13.136_901_9, 52.519_822_5)?,
        ])?])?)))
        .entity_type(&block_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(0)));
    expect_that!(page_4.items(), len(eq(0)));
    // 019_11_08.
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::contains(Shape::Point(Position::try_new(13.344_268_8, 52.465_004_5)?)))
        .entity_type(&block_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(1)));
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&block_2_id)));
    // A Point cannot contain a Polygon.
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::contains(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(2.0, 0.0)?,
            Position::try_new(2.0, 2.0)?,
            Position::try_new(0.0, 2.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    Ok(())
}
