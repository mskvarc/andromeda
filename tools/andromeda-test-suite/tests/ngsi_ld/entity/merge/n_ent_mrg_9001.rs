//! Merge entity replaces an array with the empty array.

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
    model::{Entity, EntityFragment, NgsiLdMediaType, Value},
};

/// Pins: Merge Entity writing the empty array as the value of a Property holding two strings answers 204, and the
/// Property then holds the empty array, an array being no JSON object to merge into
/// Clause: 5.6.17, 5.5.12
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9001_merge_entity_replaces_an_array_with_the_empty_array() -> Result<()> {
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
    let fragment = EntityFragment::builder().property("tags", Value::Array(vec![])).build()?;
    broker.entities().merge(&empty_probe_id, &fragment).send().await?.done()?;
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
