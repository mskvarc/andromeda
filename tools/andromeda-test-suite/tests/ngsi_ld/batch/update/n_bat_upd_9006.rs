//! A batch fragment that changes a value moves the entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_context_list,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::SystemAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Update and then Batch Entity Merge of an entity with a new speed each answer 204, store the
/// speed and move the entity's `modifiedAt`
/// Clause: 5.6.9.4, 5.6.20.4, 4.8
#[gtest]
#[tokio::test]
async fn n_bat_upd_9006_a_batch_fragment_that_changes_a_value_moves_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .property("brandName", "Mercedes")
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
    let before = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 60)
        .build()?];
    core_context_list_body.entities().batch_update(&fragments).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, not(eq(before)));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(60)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    let before_2 = system_time::of_entity(entity_3, SystemMember::ModifiedAt)?;
    let fragments_2 = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 70)
        .build()?];
    core_context_list_body.entities().batch_merge(&fragments_2).send().await?.done()?;
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(system_time::of_entity(entity_4, SystemMember::ModifiedAt)?, not(eq(before_2)));
    expect_that!(entity_4, has_attribute("speed", any_attribute().value(70)));
    Ok(())
}
