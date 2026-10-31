//! Inline nests a chain of five hundred linked entities.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::{chain_entity, chain_length},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, Input, NgsiLdMediaType},
};
use std::iter;

/// Pins: Retrieve Entity with join=inline and `joinLevel=500` over a chain of 500 Relationships answers the whole
/// chain, each of the 500 Linked Entities nested under the Relationship before it, down to the last
/// Clause: 4.5.23.1, 4.5.23.2, 5.7.1.4, 6.5.3.1
/// Recorded: 4.5.23.1 asks a broker "to limit retrieval to avoid cascades of an excessive length" through
/// `joinLevel`, and Table 6.5.3.1-1 sets no upper bound on it; whether a broker may cap the depth below what the
/// client asks for is left open. Andromeda walks any depth the client asks for
#[gtest]
#[tokio::test]
async fn o_lnk_ret_0013_inline_nests_a_chain_of_five_hundred_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0_id = namespace.urn("Building", "0")?;
    let building_type = namespace.entity_type("Building")?;
    let building_9_id = namespace.urn("Building", 9)?;
    let building_10_id = namespace.urn("Building", 10)?;
    let building_99_id = namespace.urn("Building", 99)?;
    let building_100_id = namespace.urn("Building", 100)?;
    let building_500_id = namespace.urn("Building", 500)?;
    let building_499_id = namespace.urn("Building", 499)?;
    let entities = (0..=8)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", n)?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", n + 1)?)
                .build()?)
        })
        .chain(iter::once(Ok(Entity::builder()
            .id(building_9_id)
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_10_id)
            .build()?)))
        .chain((10..=98).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:02}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:02}", n + 1))?)
                .build()?)
        }))
        .chain(iter::once(Ok(Entity::builder()
            .id(building_99_id)
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_100_id)
            .build()?)))
        .chain((100..=499).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:03}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:03}", n + 1))?)
                .build()?)
        }))
        .chain(iter::once(Ok(Entity::builder()
            .id(building_500_id.clone())
            .entity_type(building_type.clone())
            .build()?)))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_0_id)
        .join(InlineJoin::Inline)
        .join_level(500.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(chain_length(entity, "linkedTo"), eq(500));
    expect_that!(chain_entity(entity, "linkedTo", 499), some(has_id(&building_499_id)));
    expect_that!(chain_entity(entity, "linkedTo", 500), some(has_id(&building_500_id)));
    Ok(())
}
