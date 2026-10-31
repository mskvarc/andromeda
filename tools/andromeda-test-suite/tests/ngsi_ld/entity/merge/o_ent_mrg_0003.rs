//! A merged decimal keeps every written digit.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Value},
};

/// Pins: a Property merged with the JSON number 0.110000000000000005 reads back with exactly those digits, not
/// rounded to a double
/// Clause: 5.6.17.4, 4.5.2
/// Recorded: the spec does not say how many digits of a JSON number a broker keeps (RFC 8259 leaves the precision
/// to the implementation); Andromeda keeps a written decimal exactly
#[gtest]
#[tokio::test]
async fn o_ent_mrg_0003_a_merged_decimal_keeps_every_written_digit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .property("reading", Value::Number("0.110000000000000005".parse()?))
        .build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"value\":0.110000000000000005"));
    let retrieved = &received.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("reading", property()));
    Ok(())
}
