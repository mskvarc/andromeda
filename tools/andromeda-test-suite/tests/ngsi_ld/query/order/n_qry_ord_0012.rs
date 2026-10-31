//! A distance ordering measures horizontally and ignores altitude.

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
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: sorting by distance from a reference ties three Points at one longitude and latitude and different
/// altitudes ahead of a Point 34 m aside, whether `orderFrom` carries an altitude or not
/// Clause: 4.23.1, 4.23.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_0012_a_distance_ordering_measures_horizontally_and_ignores_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0_id = namespace.urn("Building", "0")?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_0_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.400_502, 52.52, 0.0)?))
            .build()?,
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 200.0)?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 400.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // A three-ordinate reference.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), nth(3, has_id(&building_0_id)));
    // A two-ordinate reference.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(4)));
    expect_that!(page_2.items(), nth(3, has_id(&building_0_id)));
    Ok(())
}
