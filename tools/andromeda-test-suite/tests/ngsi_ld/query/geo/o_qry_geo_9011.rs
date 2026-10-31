//! Near measures on the wgs 84 spheroid.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: A Point 5000.17 m north of the origin on the WGS 84 spheroid (5028.24 m on a sphere) matches
/// near;maxDistance==5014 and not near;minDistance==5014
/// Clause: 4.10
/// Recorded: 4.10 measures distance in metres and RFC 7946 places every position on WGS 84; the spheroid as the
/// earth model is the reading Andromeda takes
#[gtest]
#[tokio::test]
async fn o_qry_geo_9011_near_measures_on_the_wgs_84_spheroid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let near_stop_north_of_origin_id = namespace.urn("NearStop", "north-of-origin")?;
    let near_stop_type = namespace.entity_type("NearStop")?;
    let near_stop_north_of_origin = Entity::builder()
        .id(near_stop_north_of_origin_id.clone())
        .entity_type(near_stop_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(0.0, 0.04522)?))
        .build()?;
    broker.entities().create(&near_stop_north_of_origin).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(5014, Shape::Point(Position::try_new(0.0, 0.0)?)))
        .entity_type(&near_stop_type)
        .ids(vec![near_stop_north_of_origin_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&near_stop_north_of_origin_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(5014, Shape::Point(Position::try_new(0.0, 0.0)?)))
        .entity_type(&near_stop_type)
        .ids(vec![near_stop_north_of_origin_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
