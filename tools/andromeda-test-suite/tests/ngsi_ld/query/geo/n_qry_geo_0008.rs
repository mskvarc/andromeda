//! Disjoint selects the entities sharing no point with the reference.

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

/// Pins: Georel=disjoint selects only the entities whose `GeoProperty` shares no point with the reference geometry:
/// a point inside or on the boundary is excluded
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel disjoint maps to Disjoint, a.Relate(b, "FF*FF****"), which needs the interiors and boundaries
/// to share no point, so a point inside or on the boundary of the reference polygon is not disjoint from it
#[gtest]
#[tokio::test]
async fn n_qry_geo_0008_disjoint_selects_the_entities_sharing_no_point_with_the_reference() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let block_id = namespace.urn("Block", 1)?;
    let block_type = namespace.entity_type("Block")?;
    let block_2_id = namespace.urn("Block", 2)?;
    let spot_overlap_id = namespace.urn("Spot", "overlap")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_far_id = namespace.urn("Spot", "far")?;
    let spot_edge_id = namespace.urn("Spot", "edge")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Berlin Brandenburg International Airport")
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
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
            .id(spot_overlap_id)
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(spot_far_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(10.0, 10.0)?))
            .build()?,
        Entity::builder()
            .id(spot_edge_id)
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(1.0, 0.5)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The inside point and the point on the edge x=1 are not disjoint.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::disjoint(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&spot_far_id)));
    // 019_10_05.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::disjoint(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.161_621_1, 52.543_812_4)?,
            Position::try_new(13.597_641, 52.484_022_4)?,
            Position::try_new(13.594_894_4, 52.340_278_2)?,
            Position::try_new(13.142_395, 52.386_428)?,
            Position::try_new(13.161_621_1, 52.543_812_4)?,
        ])?])?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    // 019_10_06.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::disjoint(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(2)));
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&building_2_id)));
    expect_that!(page_3.items(), contains(has_id(&building_3_id)));
    // 019_11_05.
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::disjoint(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
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
    expect_that!(page_4.total(), some(eq(2)));
    expect_that!(page_4.items(), len(eq(2)));
    expect_that!(page_4.items(), contains(has_id(&block_id)));
    expect_that!(page_4.items(), contains(has_id(&block_2_id)));
    Ok(())
}
