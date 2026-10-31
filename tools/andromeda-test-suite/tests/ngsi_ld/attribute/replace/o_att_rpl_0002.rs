//! A replaced decimal keeps every written digit.

use andromeda_test_suite::{broker, entity_matcher::has_instances, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: a default Property instance replaced with the JSON number 0.150000000000000005 reads back with exactly
/// those digits, not rounded to a double
/// Clause: 5.6.19.4, 4.5.2
/// Recorded: the spec does not say how many digits of a JSON number a broker keeps (RFC 8259 leaves the precision
/// to the implementation); Andromeda keeps a written decimal exactly
#[gtest]
#[tokio::test]
async fn o_att_rpl_0002_a_replaced_decimal_keeps_every_written_digit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .relationship("isParked", off_street_parking_downtown1_id)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(Value::Number("0.150000000000000005".parse()?)).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"value\":0.150000000000000005"));
    let retrieved = &received.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    Ok(())
}
