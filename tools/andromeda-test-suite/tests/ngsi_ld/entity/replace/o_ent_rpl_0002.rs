//! A replaced decimal keeps every written digit.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: an entity replaced with a Property holding the JSON number 0.120000000000000005 reads back with exactly
/// those digits, not rounded to a double
/// Clause: 5.6.18.4, 4.5.2
/// Recorded: the spec does not say how many digits of a JSON number a broker keeps (RFC 8259 leaves the precision
/// to the implementation); Andromeda keeps a written decimal exactly
#[gtest]
#[tokio::test]
async fn o_ent_rpl_0002_a_replaced_decimal_keeps_every_written_digit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let building_replacement = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("reading", Value::Number("0.120000000000000005".parse()?))
        .build()?;
    broker.entities().replace(&building_replacement).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"value\":0.120000000000000005"));
    let retrieved = &received.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("reading", property()));
    Ok(())
}
