//! A read without sys attrs hides the attribute system times.

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

/// Pins: Retrieve Entity without `sysAttrs` answers the Property without `createdAt` and `modifiedAt`, and with
/// `sysAttrs` answers its `createdAt`
/// Clause: Table 5.2.14-1, 4.8, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9002_a_read_without_sys_attrs_hides_the_attribute_system_times() -> Result<()> {
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
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(system_time::of_attribute(entity, &["name"], SystemMember::CreatedAt).ok(), none());
    expect_that!(system_time::of_attribute(entity, &["name"], SystemMember::ModifiedAt).ok(), none());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::CreatedAt).ok(), some(anything()));
    Ok(())
}
