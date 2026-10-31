//! A single target relationship nests one entity and flat agrees.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::{linked_entities, linked_entity_alone},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join inline and `joinLevel` 1 of an Entity whose Relationship names one target as a
/// bare URI nests that target as one Entity object, and join flat answers the Entity and that target
/// Clause: 4.5.23.2, 4.5.23.3, 4.5.3.2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9001_a_single_target_relationship_nests_one_entity_and_flat_agrees() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_one_a_id = namespace.urn("Building", "one-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_one_root_id = namespace.urn("Building", "one-root")?;
    let building_one_a = Entity::builder()
        .id(building_one_a_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Alice")
        .build()?;
    broker.entities().create(&building_one_a).send().await?.done()?;
    let building_one_root = Entity::builder()
        .id(building_one_root_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", building_one_a_id.clone())
        .build()?;
    broker.entities().create(&building_one_root).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_one_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(linked_entity_alone(entity, &["linkedTo"]), eq(true));
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(0, has_id(&building_one_a_id)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_one_root_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities = retrieved_2.body();
    expect_that!(entities.as_slice(), len(eq(2)));
    expect_that!(entities.as_slice(), contains(has_id(&building_one_a_id)));
    expect_that!(entities.as_slice(), contains(has_id(&building_one_root_id)));
    Ok(())
}
