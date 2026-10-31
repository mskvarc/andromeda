//! Near measures a line from its nearest edge.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
    query::geoquery,
};

/// Pins: A `LineString` whose nearest vertex is 316.70 m from the reference point and whose first edge passes
/// 100.06 m from it matches near;maxDistance==150
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9007_near_measures_a_line_from_its_nearest_edge() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_road_east_line_id = namespace.urn("NearRoad", "east-line")?;
    let near_road_type = namespace.entity_type("NearRoad")?;
    let near_road_east_line = Entity::builder()
        .id(near_road_east_line_id.clone())
        .entity_type(near_road_type.clone())
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![
                Position::try_new(8.808_693, 53.0731)?,
                Position::try_new(8.808_693, 53.0785)?,
                Position::try_new(8.817_693, 53.0785)?,
            ])?),
        )
        .build()?;
    broker.entities().create(&near_road_east_line).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(150, Shape::Point(Position::try_new(8.8072, 53.0758)?)))
        .entity_type(&near_road_type)
        .ids(vec![near_road_east_line_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_road_east_line_id)));
    Ok(())
}
