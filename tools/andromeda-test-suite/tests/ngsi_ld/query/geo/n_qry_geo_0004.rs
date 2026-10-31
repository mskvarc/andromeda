//! Within selects the entities inside the reference geometry.

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

/// Pins: Georel=within selects the entities whose `GeoProperty` lies inside the reference geometry: a point on the
/// boundary or a polygon partly outside does not
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel within maps to Within, a.Relate(b, "T*F**F***"), which needs I(a) and I(b) to meet, so a point
/// exactly on the boundary of the reference polygon is not within it, and a polygon with any part in E(b) is not
/// within it
#[gtest]
#[tokio::test]
async fn n_qry_geo_0004_within_selects_the_entities_inside_the_reference_geometry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let block_id = namespace.urn("Block", 1)?;
    let block_type = namespace.entity_type("Block")?;
    let block_2_id = namespace.urn("Block", 2)?;
    let landmark_id = namespace.urn("Landmark", 1)?;
    let landmark_type = namespace.entity_type("Landmark")?;
    let landmark_2_id = namespace.urn("Landmark", 2)?;
    let landmark_3_id = namespace.urn("Landmark", 3)?;
    let spot_in_id = namespace.urn("Spot", "in")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_out_id = namespace.urn("Spot", "out")?;
    let buoy_id = namespace.urn("Buoy", 1)?;
    let buoy_type = namespace.entity_type("Buoy")?;
    let lot_edge_id = namespace.urn("Lot", "edge")?;
    let lot_type = namespace.entity_type("Lot")?;
    let lot_straddle_id = namespace.urn("Lot", "straddle")?;
    let lot_inside_id = namespace.urn("Lot", "inside")?;
    let entities = vec![
        Entity::builder()
            .id(building_id)
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
            .id(landmark_id)
            .entity_type(landmark_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(landmark_2_id)
            .entity_type(landmark_type.clone())
            .property("name", "Brandenburg Gate")
            .geo_property("location", Shape::Point(Position::try_new(13.38893, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(landmark_3_id.clone())
            .entity_type(landmark_type.clone())
            .property("name", "Victory Column")
            .geo_property("location", Shape::Point(Position::try_new(13.35012, 52.51434)?))
            .build()?,
        Entity::builder()
            .id(spot_in_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .build()?,
        Entity::builder()
            .id(spot_out_id)
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(20.0, 40.0)?))
            .build()?,
        Entity::builder()
            .id(buoy_id)
            .entity_type(buoy_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.0, 0.0)?))
            .build()?,
        Entity::builder()
            .id(lot_edge_id)
            .entity_type(lot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.0, 1.0)?))
            .build()?,
        Entity::builder()
            .id(lot_straddle_id)
            .entity_type(lot_type.clone())
            .geo_property(
                "location",
                Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                    Position::try_new(1.0, 0.5)?,
                    Position::try_new(3.0, 0.5)?,
                    Position::try_new(3.0, 1.5)?,
                    Position::try_new(1.0, 1.5)?,
                    Position::try_new(1.0, 0.5)?,
                ])?])?),
            )
            .build()?,
        Entity::builder()
            .id(lot_inside_id.clone())
            .entity_type(lot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(1.0, 1.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Only the Victory Column lies below the polygon's northern edge.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.161_621_1, 52.543_812_4)?,
            Position::try_new(13.597_641, 52.484_022_4)?,
            Position::try_new(13.594_894_4, 52.340_278_2)?,
            Position::try_new(13.142_395, 52.386_428)?,
            Position::try_new(13.161_621_1, 52.543_812_4)?,
        ])?])?)))
        .entity_type(&landmark_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&landmark_3_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.0, 52.0)?,
            Position::try_new(14.0, 52.0)?,
            Position::try_new(14.0, 53.0)?,
            Position::try_new(13.0, 53.0)?,
            Position::try_new(13.0, 52.0)?,
        ])?])?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&spot_in_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(10.0, 10.0)?,
            Position::try_new(11.0, 10.0)?,
            Position::try_new(11.0, 11.0)?,
            Position::try_new(10.0, 11.0)?,
            Position::try_new(10.0, 10.0)?,
        ])?])?)))
        .entity_type(&buoy_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
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
    expect_that!(page_4.total(), some(eq(2)));
    expect_that!(page_4.items(), len(eq(2)));
    expect_that!(page_4.items(), contains(has_id(&building_2_id)));
    expect_that!(page_4.items(), contains(has_id(&building_3_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.302_383_4, 52.490_306_6)?,
            Position::try_new(13.274_574_3, 52.465_632)?,
            Position::try_new(13.334_312_4, 52.418_544_7)?,
            Position::try_new(13.431_129_5, 52.47107)?,
            Position::try_new(13.302_383_4, 52.490_306_6)?,
        ])?])?)))
        .entity_type(&block_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(1)));
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&block_2_id)));
    // A point on the edge x=0 and a polygon straddling the edge x=2 are not within.
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(2.0, 0.0)?,
            Position::try_new(2.0, 2.0)?,
            Position::try_new(0.0, 2.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&lot_type)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&lot_inside_id)));
    Ok(())
}
