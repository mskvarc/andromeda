//! A distance ordering measures the geo property it names.

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

/// Pins: sorting by distance on `observationPoint` orders by that `GeoProperty` and not by location, ascending and
/// descending, though location orders the entities the other way round
/// Clause: 4.23.1, 4.23.3, 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_0013_a_distance_ordering_measures_the_geo_property_it_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.6, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.5, 52.52)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.9, 52.52)?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.9, 52.52)?))
            .geo_property("observationPoint", Shape::Point(Position::try_new(13.4, 52.52)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("observationPoint;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_3_id)));
    expect_that!(page.items(), nth(1, has_id(&building_id)));
    expect_that!(page.items(), nth(2, has_id(&building_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("observationPoint;dist-desc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_id)));
    expect_that!(page_2.items(), nth(2, has_id(&building_3_id)));
    Ok(())
}
