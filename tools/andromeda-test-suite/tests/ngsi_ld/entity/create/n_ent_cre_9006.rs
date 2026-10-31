//! A property value may be the empty array.

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
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Create Entity with a Property whose value is the empty array answers 201, and the Entity reads back with
/// that Property holding the empty array
/// Clause: 5.6.1, Table 5.2.5-1, 3.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9006_a_property_value_may_be_the_empty_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_id = namespace.urn("EmptyProbe", 1)?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let empty_probe = Entity::builder()
        .id(empty_probe_id.clone())
        .entity_type(empty_probe_type)
        .property("tags", Value::Array(vec![]))
        .build()?;
    broker.entities().create(&empty_probe).send().await?.done()?;
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
