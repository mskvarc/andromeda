//! A geo-query finds an entity created just before it.

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

/// Pins: Query Entities of a Spot type with near;maxDistance==1000 around (13.38893, 52.534473) answers no Spot, and
/// sent at once after a Spot is created at that point answers that Spot
/// Clause: 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9099_a_geo_query_finds_an_entity_created_just_before_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_type = namespace.entity_type("Spot")?;
    let fresh_id = namespace.urn("Spot", "fresh")?;
    let reference = Shape::Point(Position::try_new(13.38893, 52.534_473)?);
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let before = json
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference.clone()))
        .send()
        .await?;
    expect_that!(before.items(), len(eq(0)));
    let fresh = Entity::builder()
        .id(fresh_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", reference.clone())
        .build()?;
    broker.entities().create(&fresh).send().await?.done()?;
    let after = json
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference))
        .send()
        .await?;
    expect_that!(after.items(), len(eq(1)));
    expect_that!(after.items(), nth(0, has_id(&fresh_id)));
    Ok(())
}
