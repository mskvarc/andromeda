//! Inline gives a list relationship of three every linked entity in object list order.

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
    http::headers::Accept,
    model::{Entity, ListRelationship, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join=inline and `joinLevel=1` gives a `ListRelationship` of three objects an
/// `entityList` holding all three Linked Entities in `objectList` order, which is not their identifier order
/// Clause: 4.5.23.2, 4.5.22, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0014_inline_gives_a_list_relationship_of_three_every_linked_entity_in_object_list_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let device_a_id = namespace.urn("Device", "a")?;
    let device_b_id = namespace.urn("Device", "b")?;
    let device_c_id = namespace.urn("Device", "c")?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let entities = [
        Entity::builder()
            .id(device_a_id.clone())
            .entity_type(device_type.clone())
            .property("name", "DevA")
            .build()?,
        Entity::builder()
            .id(device_b_id.clone())
            .entity_type(device_type.clone())
            .property("name", "DevB")
            .build()?,
        Entity::builder()
            .id(device_c_id.clone())
            .entity_type(device_type.clone())
            .property("name", "DevC")
            .build()?,
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .list_relationship(
                "hasDevices",
                ListRelationship::builder(vec![device_c_id.clone(), device_a_id.clone(), device_b_id.clone()]),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_attribute("hasDevices", any_attribute().list_length(3))));
    let building = page.items().first().or_fail()?;
    let devices = linked_entities(building, &["hasDevices"]);
    expect_that!(devices, len(eq(3)));
    expect_that!(devices, nth(0, has_id(&device_c_id)));
    expect_that!(devices, nth(0, has_attribute("name", property().value("DevC"))));
    expect_that!(devices, nth(1, has_id(&device_a_id)));
    expect_that!(devices, nth(1, has_attribute("name", property().value("DevA"))));
    expect_that!(devices, nth(2, has_id(&device_b_id)));
    expect_that!(devices, nth(2, has_attribute("name", property().value("DevB"))));
    Ok(())
}
