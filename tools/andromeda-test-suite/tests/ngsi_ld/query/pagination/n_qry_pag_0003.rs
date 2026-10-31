//! Count honours a q and a near geo query.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{
        Entity,
        NgsiLdMediaType,
        Property,
        geometry::{Position, Shape},
    },
    query::{geoquery, q},
};

/// Pins: NGSILD-Results-Count counts the entities that pass every filter, a q value test and a near geo-query, not
/// every entity of the type
/// Clause: 6.3.13, 4.9, 4.10, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_pag_0003_count_honours_a_q_and_a_near_geo_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let station_0_id = namespace.urn("Station", "0")?;
    let station_type = namespace.entity_type("Station")?;
    let station_id = namespace.urn("Station", 1)?;
    let station_2_id = namespace.urn("Station", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(6).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
            )
            .property("almostFull", true)
            .build()?,
        Entity::builder()
            .id(station_0_id.clone())
            .entity_type(station_type.clone())
            .property("name", "b0")
            .geo_property("location", Shape::Point(Position::try_new(13.38893, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(station_id.clone())
            .entity_type(station_type.clone())
            .property("name", "b1")
            .geo_property("location", Shape::Point(Position::try_new(13.38993, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(station_2_id.clone())
            .entity_type(station_type.clone())
            .property("name", "b2")
            .geo_property("location", Shape::Point(Position::try_new(13.39093, 52.534_473)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("airQualityLevel").eq(6))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.388_930, 52.534_473)?)))
        .entity_type(&station_type)
        .ids(vec![station_0_id.clone(), station_id.clone(), station_2_id.clone()])
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(3)));
    expect_that!(page_2.items(), len(eq(3)));
    Ok(())
}
