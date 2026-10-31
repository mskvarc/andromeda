//! Near reaches a long edge of the location where it bows along its great circle.

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

/// Pins: an Entity whose location is the `LineString` (-30,60) to (30,60) matches near;maxDistance==1000 around (0,63.43) and not near;maxDistance==500; the great circle through (-30,60) and (30,60) bows north to 63.43, so (0,63.43) is 551.64 m from the edge although it lies 380 km north of both vertices
/// Clause: 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9090_near_reaches_a_long_edge_of_the_location_where_it_bows_along_its_great_circle() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let route_id = namespace.urn("Route", "default")?;
    let route_type = namespace.entity_type("Route")?;
    let route = Entity::builder()
        .id(route_id.clone())
        .entity_type(route_type.clone())
        .geo_property(
            "location",
            Shape::LineString(LineString::try_new(vec![Position::try_new(-30.0, 60.0)?, Position::try_new(30.0, 60.0)?])?),
        )
        .build()?;
    broker.entities().create(&route).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(0.0, 63.43)?)))
        .entity_type(&route_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&route_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(500, Shape::Point(Position::try_new(0.0, 63.43)?)))
        .entity_type(&route_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
