//! The reference point wins over a geo query around another point.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{
        Locality,
        geoquery,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: a distance ordering measures from `orderFrom` even when the request also carries a geo-query around
/// another point; the geo-query only selects the entities
/// Clause: 4.23.3, 4.10, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ord_0003_the_reference_point_wins_over_a_geo_query_around_another_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_4_id = namespace.urn("Building", 4)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Gamma")
            .geo_property("location", Shape::Point(Position::try_new(2.3522, 48.8566)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Alpha")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Beta")
            .geo_property("location", Shape::Point(Position::try_new(8.6821, 50.1109)?))
            .build()?,
        Entity::builder()
            .id(building_4_id)
            .entity_type(building_type.clone())
            .property("name", "Delta")
            .geo_property("location", Shape::Point(Position::try_new(-73.9857, 40.7484)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The geo-query around Paris keeps the three European entities and drops the fourth; the order is by distance from Berlin.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(1_200_000, Shape::Point(Position::try_new(2.3522, 48.8566)?)))
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page.items(), nth(2, has_id(&building_id)));
    Ok(())
}
