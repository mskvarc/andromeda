//! Relationships read back with their objects.

use andromeda_test_suite::{attribute_matcher::relationship, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Creation of two entities with a Relationship answers 201, and each entity reads back with
/// that Relationship and its object
/// Clause: 5.6.7.4, 6.14.3.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_bat_cre_0003_relationships_read_back_with_their_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_pisa_id = namespace.urn("City", "Pisa")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_pisa_id.clone())
            .build()?,
    ];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, len(eq(2)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("locatedAt", relationship().object(city_paris_id.clone())));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("locatedAt", relationship().object(city_pisa_id.clone())));
    Ok(())
}
