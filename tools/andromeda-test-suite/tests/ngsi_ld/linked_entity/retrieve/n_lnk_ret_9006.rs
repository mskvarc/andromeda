//! A multi target relationship simplifies to an array of key value entities.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, simplified_matcher::has_value, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, RepresentationSpelling},
    },
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Entity with join inline, `joinLevel` 1 and `keyValues` of an Entity whose Relationship names two
/// targets renders that Relationship as an array of the two targets, each a key-value Entity holding only id, type
/// and name
/// Clause: 4.5.4, 4.5.23.2
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9006_a_multi_target_relationship_simplifies_to_an_array_of_key_value_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "simple-a")?;
    let building_b_id = namespace.urn("Building", "simple-b")?;
    let building_root_id = namespace.urn("Building", "simple-root")?;
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Alice")
        .build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    let building_b = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Bob")
        .build()?;
    broker.entities().create(&building_b).send().await?.done()?;
    let building_root = Entity::builder()
        .id(building_root_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", vec![building_a_id.clone(), building_b_id.clone()])
        .build()?;
    broker.entities().create(&building_root).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    // each target holds id, type and name only, in the order of the Relationship's objects
    expect_that!(
        retrieved.body(),
        has_value(
            "linkedTo",
            Value::from(vec![
                object([
                    ("id", Value::from(building_a_id.to_string())),
                    ("type", Value::from(building_type.as_str())),
                    ("name", Value::from("Alice")),
                ]),
                object([
                    ("id", Value::from(building_b_id.to_string())),
                    ("type", Value::from(building_type.as_str())),
                    ("name", Value::from("Bob")),
                ]),
            ])
        )
    );
    Ok(())
}
