//! Equals matches points of two and three ordinates.

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

/// Pins: georel equals matches a two-ordinate and a three-ordinate Point at the same longitude and latitude, for a
/// reference of either arity, and not a Point beside them
/// Clause: 4.10, 4.7.1, 5.7.2
/// Recorded: clause 4.10: "the target geometry shall be equal, as specified by `[14]`, to the reference geometry";
/// OGC 06-103r4 clause 6.1.15.3: a.Equals(b) <=> a within b and b within a; clause 4.10 defers the geometric
/// relationships to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations work in the "map geometry" of the
/// data and "will therefore not reflect z or m values in calculations" and whose clause 6.1.15.1 defines the
/// relationships by the projection of the objects onto the horizontal surface, so a georel that ignores the
/// altitude is the specified behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_qry_geo_0076_equals_matches_points_of_two_and_three_ordinates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let post_flat_id = namespace.urn("Post", "flat")?;
    let post_type = namespace.entity_type("Post")?;
    let post_elevated_id = namespace.urn("Post", "elevated")?;
    let post_beside_id = namespace.urn("Post", "beside")?;
    let entities = [
        Entity::builder()
            .id(post_flat_id.clone())
            .entity_type(post_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .build()?,
        Entity::builder()
            .id(post_elevated_id.clone())
            .entity_type(post_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 500.0)?))
            .build()?,
        Entity::builder()
            .id(post_beside_id)
            .entity_type(post_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.5, 52.52, 500.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // A two-ordinate reference.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .entity_type(&post_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&post_flat_id)));
    expect_that!(page.items(), contains(has_id(&post_elevated_id)));
    // A three-ordinate reference at altitude 0.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)))
        .entity_type(&post_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&post_flat_id)));
    expect_that!(page_2.items(), contains(has_id(&post_elevated_id)));
    Ok(())
}
