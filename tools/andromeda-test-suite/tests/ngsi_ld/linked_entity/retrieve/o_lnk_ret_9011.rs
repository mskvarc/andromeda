//! A version preference amends an inline chain of one thousand linked entities.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::{chain_entity, chain_length},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, Prefer, SpecVersion},
        operation::InlineJoin,
    },
    model::{Entity, Input, NgsiLdMediaType},
};
use std::iter;

/// Pins: Retrieve Entity with join=inline, `joinLevel=1000` and Prefer ngsi-ld=1.6 over a chain of 1000
/// Relationships answers 203 with Preference-Applied ngsi-ld=1.6 and every one of the 1000 Linked Entities nested
/// under the Relationship before it, down to the last
/// Clause: 4.3.6.8, 4.5.23.1, 4.5.23.2, 5.7.1.4, 6.3.6
/// Recorded: 4.5.23.1 asks a broker "to limit retrieval to avoid cascades of an excessive length" through
/// `joinLevel` and Table 6.5.3.1-1 sets no upper bound on it, and Table 6.5.3.1-3 gives 203 to "an altered response
/// body" while 4.3.6.8 removes nothing from this chain at version 1.6; Andromeda walks any depth the client asks
/// for and answers a version preference 203
#[gtest]
#[tokio::test]
async fn o_lnk_ret_9011_a_version_preference_amends_an_inline_chain_of_one_thousand_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0_id = namespace.urn("Building", "0")?;
    let building_type = namespace.entity_type("Building")?;
    let building_9_id = namespace.urn("Building", 9)?;
    let building_10_id = namespace.urn("Building", 10)?;
    let building_99_id = namespace.urn("Building", 99)?;
    let building_100_id = namespace.urn("Building", 100)?;
    let building_999_id = namespace.urn("Building", 999)?;
    let building_1000_id = namespace.urn("Building", 1000)?;
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
        .chain((100..=998).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:03}"))?)
                .entity_type(building_type.clone())
                .relationship("linkedTo", namespace.urn("Building", format!("{:03}", n + 1))?)
                .build()?)
        }))
        .chain(iter::once(Ok(Entity::builder()
            .id(building_999_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_1000_id.clone())
            .build()?)))
        .chain(iter::once(Ok(Entity::builder()
            .id(building_1000_id.clone())
            .entity_type(building_type.clone())
            .build()?)))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_prefer(Prefer::builder().version("1.6".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve(&building_0_id)
        .join(InlineJoin::Inline)
        .join_level(1000.try_into()?)
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(retrieved.preference_applied(), some(eq(&"1.6".parse::<SpecVersion>()?)));
    let entity = retrieved.body();
    expect_that!(chain_length(entity, "linkedTo"), eq(1000));
    expect_that!(chain_entity(entity, "linkedTo", 999), some(has_id(&building_999_id)));
    expect_that!(chain_entity(entity, "linkedTo", 1000), some(has_id(&building_1000_id)));
    Ok(())
}
