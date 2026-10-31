//! A repeated id is created once and replaced by the later instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::{Done, UpsertCreated},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert without options naming a new entity twice answers 201 listing each created id once,
/// and the entity holds what the later instance states and nothing the later instance omits
/// Clause: 5.5.11.0, 5.5.11.2, 5.6.8.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0015_a_repeated_id_is_created_once_and_replaced_by_the_later_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
    ];
    let created = broker.entities().batch_upsert(&entities).send().await?.done()?.created_entities()?;
    expect_that!(created, len(eq(2)));
    expect_that!(created, contains(eq(&building_id)));
    expect_that!(created, contains(eq(&building_2_id)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("name"));
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(city_paris_id.clone())));
    Ok(())
}
