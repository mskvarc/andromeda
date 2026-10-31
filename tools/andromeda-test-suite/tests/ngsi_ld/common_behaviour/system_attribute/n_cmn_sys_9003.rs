//! An update keeps created at and moves modified at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update of a Property keeps its `createdAt` and moves its `modifiedAt` away from it
/// Clause: 4.8, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9003_an_update_keeps_created_at_and_moves_modified_at() -> Result<()> {
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
    let patch = Attribute::from(Property::builder("second").build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_id, &"name".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::CreatedAt)?, eq(created));
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::ModifiedAt)?, not(eq(created)));
    expect_that!(entity_2, has_attribute("name", any_attribute().value("second")));
    Ok(())
}
