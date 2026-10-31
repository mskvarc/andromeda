//! Near min distance selects the entities beyond that distance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Near;minDistance selects exactly the entities at least that many metres from the reference point
/// Clause: 4.10, 5.7.2, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_geo_0010_near_min_distance_selects_the_entities_beyond_that_distance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let spot_inner_id = namespace.urn("Spot", "inner")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_outer_id = namespace.urn("Spot", "outer")?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Berlin Brandenburg International Airport")
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .property("name", "Grunewald")
            .geo_property("location", Shape::Point(Position::try_new(13.215_983, 52.470_441)?))
            .build()?,
        Entity::builder()
            .id(spot_inner_id)
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.38983, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(spot_outer_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.464, 52.534_473)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // About 60 m and 5.1 km away.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(1000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&spot_outer_id)));
    // 019_10_03.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(15000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_2_id)));
    Ok(())
}
