//! Near measures a polygon from its nearest wall.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
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

/// Pins: A Polygon whose nearest wall is 100.06 m from the reference point and whose centroid is 170.10 m away
/// matches near;maxDistance==150 and near;minDistance==90, and neither near;maxDistance==90 nor
/// near;minDistance==150
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9004_near_measures_a_polygon_from_its_nearest_wall() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_building_east_square_id = namespace.urn("NearBuilding", "east-square")?;
    let near_building_type = namespace.entity_type("NearBuilding")?;
    let near_building_east_square = Entity::builder()
        .id(near_building_east_square_id.clone())
        .entity_type(near_building_type.clone())
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(8.808_693, 53.075_171)?,
                Position::try_new(8.810_783, 53.075_171)?,
                Position::try_new(8.810_783, 53.076_429)?,
                Position::try_new(8.808_693, 53.076_429)?,
                Position::try_new(8.808_693, 53.075_171)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&near_building_east_square).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(150, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_east_square_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_building_east_square_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(90, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_east_square_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(150, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_east_square_id.clone()])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(90, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_east_square_id.clone()])
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&near_building_east_square_id)));
    Ok(())
}
