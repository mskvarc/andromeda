//! Each dataset instance keeps its own system times.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    dataset_instance_time::of_dataset_instance,
    entity_matcher::has_instance,
    namespace::Namespace,
    outcome::Done,
    system_time::SystemMember,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update of the instance of `datasetId` A moves its `modifiedAt` away from its
/// `createdAt`, and the instance of `datasetId` B keeps a `modifiedAt` equal to its `createdAt`
/// Clause: 4.8, 4.5.5, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9004_each_dataset_instance_keeps_its_own_system_times() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "A")?;
    let dataset_b = namespace.dataset_id("Dataset", "B")?;
    let name = "name".parse::<AttributeName>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("name", Property::builder("a").dataset_id(dataset_a.clone()))
        .property("name", Property::builder("b").dataset_id(dataset_b.clone()))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let patch = Attribute::from(Property::builder("a2").dataset_id(dataset_a.clone()).build()?).into_patch();
    broker.entities().update_attribute(&vehicle_id, &name, patch).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let a_created = of_dataset_instance(entity, &name, &dataset_a, SystemMember::CreatedAt)?;
    let b_created = of_dataset_instance(entity, &name, &dataset_b, SystemMember::CreatedAt)?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_instance("name", any_attribute().value("a2").dataset_id(dataset_a.clone())));
    expect_that!(of_dataset_instance(entity_2, &name, &dataset_a, SystemMember::ModifiedAt)?, not(eq(a_created)));
    expect_that!(of_dataset_instance(entity_2, &name, &dataset_b, SystemMember::ModifiedAt)?, eq(b_created));
    Ok(())
}
