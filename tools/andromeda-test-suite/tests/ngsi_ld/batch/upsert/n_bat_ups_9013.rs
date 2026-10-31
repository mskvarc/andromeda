//! A repeated id replaces in array order and is listed once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    nth::nth,
    outcome::{Done, UpsertCreated},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Batch Entity Upsert without options naming a new Entity twice, first with name and then with
/// `subCategory`, answers 201 listing the id once, and the Entity holds `subCategory` and no name
/// Clause: 5.5.11.0, 5.5.11.2, 5.6.8.4, 5.6.8.5
#[gtest]
#[tokio::test]
async fn n_bat_ups_9013_a_repeated_id_replaces_in_array_order_and_is_listed_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_replace_id = namespace.urn("Building", "replace")?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(building_replace_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(building_replace_id.clone())
            .entity_type(building_type.clone())
            .property("subCategory", "tourism")
            .build()?,
    ];
    let created = broker.entities().batch_upsert(&entities).send().await?.done()?.created_entities()?;
    expect_that!(created, len(eq(1)));
    expect_that!(created.as_slice(), nth(0, eq(&building_replace_id)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_replace_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("name"));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    Ok(())
}
