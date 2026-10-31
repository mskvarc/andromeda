//! Near max distance selects the entities within that distance.

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

/// Pins: Near;maxDistance selects exactly the entities within that many metres of the reference point
/// Clause: 4.10, 5.7.2, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_geo_0003_near_max_distance_selects_the_entities_within_that_distance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let landmark_id = namespace.urn("Landmark", 1)?;
    let landmark_type = namespace.entity_type("Landmark")?;
    let landmark_2_id = namespace.urn("Landmark", 2)?;
    let landmark_3_id = namespace.urn("Landmark", 3)?;
    let spot_in_id = namespace.urn("Spot", "in")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_out_id = namespace.urn("Spot", "out")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("name", "Berlin Brandenburg International Airport")
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Grunewald")
            .geo_property("location", Shape::Point(Position::try_new(13.215_983, 52.470_441)?))
            .build()?,
        Entity::builder()
            .id(landmark_id.clone())
            .entity_type(landmark_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(landmark_2_id.clone())
            .entity_type(landmark_type.clone())
            .property("name", "Brandenburg Gate")
            .geo_property("location", Shape::Point(Position::try_new(13.38893, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(landmark_3_id.clone())
            .entity_type(landmark_type.clone())
            .property("name", "Victory Column")
            .geo_property("location", Shape::Point(Position::try_new(13.35012, 52.51434)?))
            .build()?,
        Entity::builder()
            .id(spot_in_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.5, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(spot_out_id)
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(20.0, 52.5547)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // All three landmarks are within 3 km.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&landmark_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(3)));
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&landmark_id)));
    expect_that!(page.items(), contains(has_id(&landmark_2_id)));
    expect_that!(page.items(), contains(has_id(&landmark_3_id)));
    // About 7.9 km and about 450 km away.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&spot_in_id)));
    // About 2.4 km and 13.7 km; the airport is about 20.7 km away.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(2)));
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&building_id)));
    expect_that!(page_3.items(), contains(has_id(&building_3_id)));
    Ok(())
}
