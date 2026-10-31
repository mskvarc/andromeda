//! Near measures across the antimeridian.

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

/// Pins: A `MultiPolygon` cut at the antimeridian, 212.97 m from a reference point on its other side and 692.16 m
/// from it by planar centroid, matches near;maxDistance==300
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9009_near_measures_across_the_antimeridian() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_building_taveuni_id = namespace.urn("NearBuilding", "taveuni")?;
    let near_building_type = namespace.entity_type("NearBuilding")?;
    let near_building_taveuni = Entity::builder()
        .id(near_building_taveuni_id.clone())
        .entity_type(near_building_type.clone())
        .geo_property(
            "location",
            Shape::MultiPolygon(vec![Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(-179.999, -17.0005)?,
                Position::try_new(-179.99, -17.0005)?,
                Position::try_new(-179.99, -16.9995)?,
                Position::try_new(-179.999, -16.9995)?,
                Position::try_new(-179.999, -17.0005)?,
            ])?])?]),
        )
        .build()?;
    broker.entities().create(&near_building_taveuni).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(300, Shape::Point(Position::try_new(179.999, -17.0)?)))
        .entity_type(&near_building_type)
        .ids(vec![near_building_taveuni_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_building_taveuni_id)));
    Ok(())
}
