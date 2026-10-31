//! A created attribute carries equal created and modified at.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: A newly created Property reads back under `sysAttrs` with a `createdAt` and a `modifiedAt` equal to it
/// Clause: 4.8, Table 5.2.14-1, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9001_a_created_attribute_carries_equal_created_and_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("name", "first")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let created = system_time::of_attribute(entity, &["name"], SystemMember::CreatedAt)?;
    expect_that!(system_time::of_attribute(entity, &["name"], SystemMember::CreatedAt).ok(), some(anything()));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::ModifiedAt)?, eq(created));
    Ok(())
}
