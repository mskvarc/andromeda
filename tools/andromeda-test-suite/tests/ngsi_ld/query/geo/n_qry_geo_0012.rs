//! A geo query is one more condition beside type id and q.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::{geoquery, q},
};

/// Pins: A geo-query is one more condition beside type, id and q: an entity is selected only when every one of them
/// holds
/// Clause: 4.10, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0012_a_geo_query_is_one_more_condition_beside_type_id_and_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_a_id = namespace.urn("Spot", "a")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_b_id = namespace.urn("Spot", "b")?;
    let spot_c_id = namespace.urn("Spot", "c")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_3_id = namespace.urn("Sensor", 3)?;
    let entities = [
        Entity::builder()
            .id(spot_a_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(spot_b_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(spot_c_id.clone())
            .entity_type(spot_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(vehicle_id)
            .entity_type(vehicle_type)
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(sensor_id.clone())
            .entity_type(sensor_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(8.0, 40.0)?))
            .property("p", 1)
            .build()?,
        Entity::builder()
            .id(sensor_2_id)
            .entity_type(sensor_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(8.0, 40.0)?))
            .property("p", 2)
            .build()?,
        Entity::builder()
            .id(sensor_3_id)
            .entity_type(sensor_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(9.0, 41.0)?))
            .property("p", 1)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // id and the geo-query: c is inside but not listed.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&spot_type)
        .ids(vec![spot_a_id.clone(), spot_b_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&spot_a_id)));
    expect_that!(page.items(), contains(has_id(&spot_b_id)));
    // type and the geo-query: the Vehicle at the same point is of another type.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), contains(has_id(&spot_a_id)));
    expect_that!(page_2.items(), contains(has_id(&spot_b_id)));
    expect_that!(page_2.items(), contains(has_id(&spot_c_id)));
    // q and the geo-query: Sensor 2 is near but p is 2, Sensor 3 has p 1 but is about 140 km away.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(8.0, 40.0)?)))
        .entity_type(&sensor_type)
        .q(q::attr("p").eq(1))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
