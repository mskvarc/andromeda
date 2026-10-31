//! Replace attribute takes the empty array.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Replace Attribute with a Property whose value is the empty array answers 204, and the Property then holds
/// the empty array in place of the two strings it held
/// Clause: 5.6.19
#[gtest]
#[tokio::test]
async fn n_att_rpl_9002_replace_attribute_takes_the_empty_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_id = namespace.urn("EmptyProbe", 1)?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let empty_probe = Entity::builder()
        .id(empty_probe_id.clone())
        .entity_type(empty_probe_type)
        .property("tags", Value::Array(vec![Value::from("a"), Value::from("b")]))
        .build()?;
    broker.entities().create(&empty_probe).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(Value::Array(vec![])).build()?);
    broker
        .entities()
        .replace_attribute(&empty_probe_id, &"tags".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "tags"), some(eq(2)));
    expect_that!(entity, has_attribute("tags", property().length_at([], 0)));
    Ok(())
}
