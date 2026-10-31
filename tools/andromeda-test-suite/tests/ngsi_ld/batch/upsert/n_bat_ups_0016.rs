//! Update mode repeated id keeps every instance.

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

/// Pins: a Batch Entity Upsert with options=update naming a new entity twice, each instance carrying a different
/// Attribute, answers 201 and the entity holds the Attributes of both instances
/// Clause: 5.5.11.0, 5.5.11.2, 5.6.8.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0016_update_mode_repeated_id_keeps_every_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
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
    expect_that!(created.as_slice(), nth(0, eq(&building_id)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(city_paris_id.clone())));
    Ok(())
}
