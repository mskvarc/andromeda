//! A member set to the null removes every instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity and Batch Entity Merge of speed set to the NGSI-LD Null on entities holding a default, an A
/// and a B instance of speed beside brand answer 204, and both entities keep brand and no instance of speed
/// Clause: 5.6.20.4, 5.6.17.4, 5.5.12
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9002_a_member_set_to_the_null_removes_every_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let null_member_single_id = namespace.urn("NullMember", "single")?;
    let null_member_type = namespace.entity_type("NullMember")?;
    let null_member_batched_id = namespace.urn("NullMember", "batched")?;
    let null_member_single = Entity::builder()
        .id(null_member_single_id.clone())
        .entity_type(null_member_type.clone())
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "A")?))
        .property("speed", Property::builder(3).dataset_id(namespace.dataset_id("Dataset", "B")?))
        .property("brand", "x")
        .build()?;
    broker.entities().create(&null_member_single).send().await?.done()?;
    let null_member_batched = Entity::builder()
        .id(null_member_batched_id.clone())
        .entity_type(null_member_type.clone())
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "A")?))
        .property("speed", Property::builder(3).dataset_id(namespace.dataset_id("Dataset", "B")?))
        .property("brand", "x")
        .build()?;
    broker.entities().create(&null_member_batched).send().await?.done()?;
    let fragment = EntityFragment::builder().delete("speed").build()?;
    broker.entities().merge(&null_member_single_id, &fragment).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(null_member_batched_id.clone(), null_member_type.clone())
        .delete("speed")
        .build()?];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    // Merge Entity.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&null_member_single_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("brand", any_attribute().value("x")));
    // Batch Entity Merge does what Merge Entity does.
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&null_member_batched_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("speed"));
    expect_that!(entity_2, has_attribute("brand", any_attribute().value("x")));
    Ok(())
}
