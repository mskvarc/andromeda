//! A geo-query finds an entity moved into range just before it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Query Entities of a Spot type with near;maxDistance==1000 around (13.38893, 52.534473) answers no Spot
/// while the only Spot is located at (14.0, 52.534473), about 41 km away, and sent at once after a Merge Entity
/// moves that Spot to the reference point answers it
/// Clause: 4.10, 5.6.17.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9100_a_geo_query_finds_an_entity_moved_into_range_just_before_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_type = namespace.entity_type("Spot")?;
    let moved_id = namespace.urn("Spot", "moved")?;
    let reference = Shape::Point(Position::try_new(13.38893, 52.534_473)?);
    let moved = Entity::builder()
        .id(moved_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.0, 52.534_473)?))
        .build()?;
    broker.entities().create(&moved).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let far = json
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference.clone()))
        .send()
        .await?;
    expect_that!(far.items(), len(eq(0)));
    let move_in = EntityFragment::builder().geo_property("location", reference.clone()).build()?;
    broker.entities().merge(&moved_id, &move_in).send().await?.done()?;
    let near = json
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference))
        .send()
        .await?;
    expect_that!(near.items(), len(eq(1)));
    expect_that!(near.items(), nth(0, has_id(&moved_id)));
    Ok(())
}
