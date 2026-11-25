//! A list of one type selects nothing through a linked Entity of another type.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: Query Entities with q sensor{Device:humidity}==40 selects nothing when the Entity the weather station's
/// sensor names is a Thermometer holding humidity 40, while q sensor{humidity}==40 selects the weather station
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 calls the list "a hint for the required Entity Type", so only such Entities "need to be
/// considered". Andromeda reads the list as a restriction, the one answer a client can rely on whatever the broker
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0016_a_list_of_one_type_selects_nothing_through_a_linked_entity_of_another_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let station_id = namespace.urn("WeatherStation", 123)?;
    let station_type = namespace.entity_type("WeatherStation")?;
    let thermometer_id = namespace.urn("Thermometer", 345)?;
    let entities = [
        Entity::builder()
            .id(thermometer_id.clone())
            .entity_type(namespace.entity_type("Thermometer")?)
            .property("humidity", 40)
            .build()?,
        Entity::builder()
            .id(station_id)
            .entity_type(station_type.clone())
            .relationship("sensor", thermometer_id)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // without the list the weather station is selected, so the list alone leaves it out below
    let untyped = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&station_type)
        .q(q::attr("sensor").linked("humidity").eq(40))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(untyped.items(), len(eq(1)));
    let typed = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&station_type)
        .q(q::attr("sensor").linked_typed([&namespace.entity_type("Device")?], "humidity").eq(40))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(typed.items(), is_empty());
    Ok(())
}
