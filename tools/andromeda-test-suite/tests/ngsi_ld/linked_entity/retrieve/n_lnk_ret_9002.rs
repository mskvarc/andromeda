//! A two target relationship nests both entities and flat agrees.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join inline and `joinLevel` 1 of an Entity whose Relationship names two targets nests
/// both, each rendered in full, and join flat answers the Entity and both targets
/// Clause: 4.5.23.2, 4.5.23.3, 4.5.3.2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9002_a_two_target_relationship_nests_both_entities_and_flat_agrees() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "two-a")?;
    let building_b_id = namespace.urn("Building", "two-b")?;
    let building_root_id = namespace.urn("Building", "two-root")?;
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
        .retrieve(&building_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let linked = linked_entities(retrieved.body(), &["linkedTo"]);
    expect_that!(linked, len(eq(2)));
    expect_that!(linked, contains(has_id(&building_a_id)));
    expect_that!(linked, contains(has_id(&building_b_id)));
    expect_that!(linked, contains(all!(has_id(&building_b_id), has_attribute("name", property().value("Bob")))));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_root_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2, len(eq(3)));
    expect_that!(entities_2, contains(has_id(&building_a_id)));
    expect_that!(entities_2, contains(has_id(&building_b_id)));
    expect_that!(entities_2, contains(has_id(&building_root_id)));
    Ok(())
}
