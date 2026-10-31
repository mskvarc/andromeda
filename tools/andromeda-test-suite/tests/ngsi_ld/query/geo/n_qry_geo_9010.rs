//! Near measures from a polygon reference to its nearest edge.

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

/// Pins: A Point 90.07 m from the east edge of a reference Polygon and 140.07 m from its centre matches
/// near;maxDistance==100 against that Polygon
/// Clause: 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_9010_near_measures_from_a_polygon_reference_to_its_nearest_edge() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_stop_east_id = namespace.urn("NearStop", "east")?;
    let near_stop_type = namespace.entity_type("NearStop")?;
    let near_stop_east = Entity::builder()
        .id(near_stop_east_id.clone())
        .entity_type(near_stop_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(8.80929, 53.0758)?))
        .build()?;
    broker.entities().create(&near_stop_east).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(
            100,
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(8.806_454, 53.075_351)?,
                Position::try_new(8.807_946, 53.075_351)?,
                Position::try_new(8.807_946, 53.076_249)?,
                Position::try_new(8.806_454, 53.076_249)?,
                Position::try_new(8.806_454, 53.075_351)?,
            ])?])?),
        ))
        .entity_type(&near_stop_type)
        .ids(vec![near_stop_east_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_stop_east_id)));
    Ok(())
}
