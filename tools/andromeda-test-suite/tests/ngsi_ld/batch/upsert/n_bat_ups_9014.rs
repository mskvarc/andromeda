//! Update mode keeps the attributes of every instance of a repeated id.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Batch Entity Upsert with options=update naming a new Entity twice, first with name and then with
/// `subCategory`, answers 201 and the Entity holds both
/// Clause: 5.5.11.0, 5.5.11.2, 5.6.8.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_9014_update_mode_keeps_the_attributes_of_every_instance_of_a_repeated_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_update_id = namespace.urn("Building", "update")?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(building_update_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(building_update_id.clone())
            .entity_type(building_type.clone())
            .property("subCategory", "tourism")
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    Ok(())
}
