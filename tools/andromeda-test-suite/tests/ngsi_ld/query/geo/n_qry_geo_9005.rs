//! A point inside a polygon is at distance zero.

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

/// Pins: A reference point inside a Polygon whose centroid is 995.24 m away matches near;maxDistance==500 and not
/// near;minDistance==500
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9005_a_point_inside_a_polygon_is_at_distance_zero() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_building_around_bremen_id = namespace.urn("NearBuilding", "around-bremen")?;
    let near_building_type = namespace.entity_type("NearBuilding")?;
    let near_building_around_bremen = Entity::builder()
        .id(near_building_around_bremen_id.clone())
        .entity_type(near_building_type.clone())
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(8.8069, 53.0753)?,
                Position::try_new(8.8372, 53.0753)?,
                Position::try_new(8.8372, 53.0763)?,
                Position::try_new(8.8069, 53.0763)?,
                Position::try_new(8.8069, 53.0753)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&near_building_around_bremen).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(500, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_around_bremen_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_building_around_bremen_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(500, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_around_bremen_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
