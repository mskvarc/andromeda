//! Geoproperty selects a two dimensional custom geo property.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
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

/// Pins: geoproperty names a two-ordinate `observationPoint` as the target of a near query while the far-away
/// location is not consulted, and without geoproperty the query reads location
/// Clause: 4.10, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0038_geoproperty_selects_a_two_dimensional_custom_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let mast_id = namespace.urn("Mast", 1)?;
    let mast_type = namespace.entity_type("Mast")?;
    let mast = Entity::builder()
        .id(mast_id.clone())
        .entity_type(mast_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(30.0, 10.0)?))
        .geo_property("observationPoint", Shape::Point(Position::try_new(13.4, 52.52)?))
        .build()?;
    broker.entities().create(&mast).send().await?.done()?;
    // observationPoint is near the reference.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)).geoproperty("observationPoint"))
        .entity_type(&mast_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&mast_id)));
    // Without geoproperty the query reads location, which lies far away.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)))
        .entity_type(&mast_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
