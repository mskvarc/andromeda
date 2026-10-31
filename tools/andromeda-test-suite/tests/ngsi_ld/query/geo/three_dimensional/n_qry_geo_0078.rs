//! Near max distance ignores the altitude.

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

/// Pins: georel near with `maxDistance` measures horizontally: a Point 500 m above the reference is within 100 m of
/// it, for a reference of three or of two ordinates, and a Point 680 m aside is not
/// Clause: 4.10, 4.7.1, 5.7.2
/// Recorded: clause 4.10: for `maxDistance` "an entity ... has to be within the buffer geometric object (as defined
/// by `[14]`) given by the reference geometry"; OGC 06-103r4 clause 6.1.2.5 lists Buffer among the operations that
/// "will therefore not reflect z or m values", and leaves implementations free to add true 3D operations; clause
/// 4.10 defers the geometric relationships to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations work in
/// the "map geometry" of the data and "will therefore not reflect z or m values in calculations" and whose clause
/// 6.1.15.1 defines the relationships by the projection of the objects onto the horizontal surface, so a georel
/// that ignores the altitude is the specified behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_qry_geo_0078_near_max_distance_ignores_the_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_ground_id = namespace.urn("Building", "ground")?;
    let building_type = namespace.entity_type("Building")?;
    let building_elevated_id = namespace.urn("Building", "elevated")?;
    let building_aside_id = namespace.urn("Building", "aside")?;
    let entities = [
        Entity::builder()
            .id(building_ground_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?))
            .build()?,
        Entity::builder()
            .id(building_elevated_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 500.0)?))
            .build()?,
        Entity::builder()
            .id(building_aside_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.41, 52.52, 0.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // A three-ordinate reference.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_ground_id)));
    expect_that!(page.items(), contains(has_id(&building_elevated_id)));
    // A two-ordinate reference.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_new(13.4, 52.52)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_ground_id)));
    expect_that!(page_2.items(), contains(has_id(&building_elevated_id)));
    Ok(())
}
