//! Update mode keeps the attributes the item omits.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, UpsertCreated},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert with options=update of an existing entity and a new one answers 201 listing only the
/// new id, and the existing entity keeps the Properties the item omits beside the item's new Relationship
/// Clause: 5.6.8.4, 5.6.8.5, 5.6.2.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0010_update_mode_keeps_the_attributes_the_item_omits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_existing_id = namespace.urn("Building", "existing")?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building_new_id = namespace.urn("Building", "new")?;
    let building_existing = Entity::builder()
        .id(building_existing_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building_existing).send().await?.done()?;
    let entities = [
        Entity::builder()
            .id(building_existing_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
        Entity::builder().id(building_new_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    let created = broker
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Update)
        .send()
        .await?
        .done()?
        .created_entities()?;
    expect_that!(created, len(eq(1)));
    expect_that!(created.as_slice(), nth(0, eq(&building_new_id)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_existing_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(city_paris_id.clone())));
    Ok(())
}
