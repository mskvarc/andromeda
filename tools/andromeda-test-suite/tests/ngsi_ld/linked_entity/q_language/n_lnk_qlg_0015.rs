//! A list of one type before the compared attribute selects through a linked Entity of that type, as clause 4.9 EXAMPLE
//! 14 writes it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: Query Entities with q sensor{Device:humidity}==40 over the weather station and Device of clause 4.9 EXAMPLE
/// 14 selects the weather station whose sensor names the Device holding humidity 40
/// Clause: 4.9, 5.7.2.4
/// Recorded: the ABNF of `LinkedEntityPath` asks for two types or more and puts the list only before a further hop,
/// while EXAMPLE 14 writes one type before the compared attribute. Andromeda reads the rule with both limits lifted
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0015_a_list_of_one_type_selects_through_a_linked_entity_of_that_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let station_id = namespace.urn("WeatherStation", 123)?;
    let station_type = namespace.entity_type("WeatherStation")?;
    let device_id = namespace.urn("Device", 345)?;
    let device_type = namespace.entity_type("Device")?;
    let entities = [
        Entity::builder()
            .id(device_id.clone())
            .entity_type(device_type.clone())
            .property("humidity", 40)
            .build()?,
        Entity::builder()
            .id(station_id.clone())
            .entity_type(station_type.clone())
            .relationship("sensor", device_id)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&station_type)
        .q(q::attr("sensor").linked_typed([&device_type], "humidity").eq(40))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&station_id)));
    Ok(())
}
