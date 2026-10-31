//! An entity holding no attribute takes the upserted relationship.

use andromeda_test_suite::{attribute_matcher::relationship, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert without options that gives a Relationship to an existing entity holding no Attribute
/// and creates a new one with it answers 201, and the existing entity reads back with the Relationship
/// Clause: 5.6.8.4, 5.6.8.5, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0004_an_entity_holding_no_attribute_takes_the_upserted_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_existing_id = namespace.urn("Building", "existing")?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building_new_id = namespace.urn("Building", "new")?;
    let city_pisa_id = namespace.urn("City", "Pisa")?;
    let building_existing = Entity::builder().id(building_existing_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_existing).send().await?.done()?;
    let entities = [
        Entity::builder()
            .id(building_existing_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
        Entity::builder()
            .id(building_new_id)
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_pisa_id)
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
    expect_that!(entity, has_attribute("locatedAt", relationship().object(city_paris_id.clone())));
    Ok(())
}
