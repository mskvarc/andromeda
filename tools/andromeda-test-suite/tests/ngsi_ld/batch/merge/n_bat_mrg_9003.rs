//! A reified null deletes only the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity and Batch Entity Merge of a speed Property whose value is the NGSI-LD Null and which names no
/// `datasetId` answer 204, and both entities keep the A and B instances of speed alone
/// Clause: 5.6.20.4, 5.6.17.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9003_a_reified_null_deletes_only_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let null_instance_single_id = namespace.urn("NullInstance", "single")?;
    let null_instance_type = namespace.entity_type("NullInstance")?;
    let null_instance_batched_id = namespace.urn("NullInstance", "batched")?;
    let null_instance_single = Entity::builder()
        .id(null_instance_single_id.clone())
        .entity_type(null_instance_type.clone())
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "A")?))
        .property("speed", Property::builder(3).dataset_id(namespace.dataset_id("Dataset", "B")?))
        .property("brand", "x")
        .build()?;
    broker.entities().create(&null_instance_single).send().await?.done()?;
    let null_instance_batched = Entity::builder()
        .id(null_instance_batched_id.clone())
        .entity_type(null_instance_type.clone())
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "A")?))
        .property("speed", Property::builder(3).dataset_id(namespace.dataset_id("Dataset", "B")?))
        .property("brand", "x")
        .build()?;
    broker.entities().create(&null_instance_batched).send().await?.done()?;
    let fragment = EntityFragment::builder().delete_kind("speed", AttributeKind::Property).build()?;
    broker.entities().merge(&null_instance_single_id, &fragment).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(null_instance_batched_id.clone(), null_instance_type.clone())
        .delete_kind("speed", AttributeKind::Property)
        .build()?];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    // Merge Entity.
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&null_instance_single_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(namespace.dataset_id("Dataset", "A")?).value(2))
    );
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(namespace.dataset_id("Dataset", "B")?).value(3))
    );
    // Batch Entity Merge does what Merge Entity does.
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&null_instance_batched_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_instances("speed", 2));
    expect_that!(
        entity_2,
        has_attribute("speed", any_attribute().dataset_id(namespace.dataset_id("Dataset", "A")?).value(2))
    );
    expect_that!(
        entity_2,
        has_attribute("speed", any_attribute().dataset_id(namespace.dataset_id("Dataset", "B")?).value(3))
    );
    Ok(())
}
