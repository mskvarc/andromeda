//! Inline gives a list relationship an entity list in object list order.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, ListRelationship, NgsiLdMediaType, OneOrMany},
};

/// Pins: Retrieve Entity with join=inline and `joinLevel=1` gives a `ListRelationship` an `entityList` holding its
/// Linked Entities in `objectList` order, beside the `objectList`
/// Clause: 5.7.1.4, 4.5.23.2, 4.5.22
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0008_inline_gives_a_list_relationship_an_entity_list_in_object_list_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let device_a_id = namespace.urn("Device", "a")?;
    let device_z_id = namespace.urn("Device", "z")?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let entities = [
        Entity::builder()
            .id(device_a_id.clone())
            .entity_type(device_type.clone())
            .property("name", "DevA")
            .build()?,
        Entity::builder()
            .id(device_z_id.clone())
            .entity_type(device_type.clone())
            .property("name", "DevZ")
            .build()?,
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .list_relationship("hasDevices", ListRelationship::builder(vec![device_z_id.clone(), device_a_id.clone()]))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("hasDevices", any_attribute().list_length(2)));
    let devices = linked_entities(entity, &["hasDevices"]);
    expect_that!(devices, len(eq(2)));
    expect_that!(devices, nth(0, has_id(&device_z_id)));
    expect_that!(
        devices.first().map(|device| &device.entity_type),
        some(eq(&Some(OneOrMany::one(device_type.clone()))))
    );
    expect_that!(devices, nth(0, has_attribute("name", property().value("DevZ"))));
    expect_that!(devices, nth(1, has_id(&device_a_id)));
    expect_that!(
        devices.get(1).map(|device| &device.entity_type),
        some(eq(&Some(OneOrMany::one(device_type.clone()))))
    );
    expect_that!(devices, nth(1, has_attribute("name", property().value("DevA"))));
    Ok(())
}
