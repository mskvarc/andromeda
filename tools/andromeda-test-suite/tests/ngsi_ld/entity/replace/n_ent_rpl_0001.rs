//! The entity takes the new attributes and keeps its created at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Replace Entity answers 204, the entity then holds exactly the new Attributes (an Attribute the replacement
/// leaves out is gone) and its `createdAt` is unchanged
/// Clause: 5.6.18.4, 6.5.3.3, 4.8
#[gtest]
#[tokio::test]
async fn n_ent_rpl_0001_the_entity_takes_the_new_attributes_and_keeps_its_created_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_pisa_id = namespace.urn("City", "Pisa")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let created = system_time::of_entity(entity, SystemMember::CreatedAt)?;
    expect_that!(system_time::of_entity(entity, SystemMember::CreatedAt).ok(), some(anything()));
    let building_replacement = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Pisa Tower")
        .relationship("locatedAt", city_pisa_id.clone())
        .build()?;
    let received = broker.with_responses().entities().replace(&building_replacement).send().await?;
    received.outcome.done()?;
    expect_that!(received.body.len(), eq(0));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::CreatedAt)?, eq(created));
    expect_that!(entity_2, lacks_attribute("subCategory"));
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Pisa Tower")));
    expect_that!(entity_2, has_attribute("locatedAt", any_attribute().object(city_pisa_id.clone())));
    Ok(())
}
