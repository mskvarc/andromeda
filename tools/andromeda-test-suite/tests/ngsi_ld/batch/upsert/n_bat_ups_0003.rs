//! An entity holding no attribute takes the upserted properties.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert without options that gives Properties to an existing entity holding no Attribute and
/// creates a new one answers 201, and both entities read back with those Properties
/// Clause: 5.6.8.4, 5.6.8.5, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0003_an_entity_holding_no_attribute_takes_the_upserted_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_existing_id = namespace.urn("Building", "existing")?;
    let building_type = namespace.entity_type("Building")?;
    let building_new_id = namespace.urn("Building", "new")?;
    let building_existing = Entity::builder().id(building_existing_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_existing).send().await?.done()?;
    let entities = [
        Entity::builder()
            .id(building_existing_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .build()?,
        Entity::builder()
            .id(building_new_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_existing_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_new_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Pisa Tower")));
    Ok(())
}
