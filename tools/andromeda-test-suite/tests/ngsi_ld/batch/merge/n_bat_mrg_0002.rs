//! A new relationship is merged beside the existing properties.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: a Batch Entity Merge adding a Relationship to two entities holding Properties answers 204, and each entity
/// reads back with the Relationship and its Properties unchanged
/// Clause: 5.6.20.4, 5.6.20.5, 5.5.12
#[gtest]
#[tokio::test]
async fn n_bat_mrg_0002_a_new_relationship_is_merged_beside_the_existing_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let city_pisa_id = namespace.urn("City", "Pisa")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("subCategory", "religion")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
        EntityFragment::for_entity(building_2_id.clone(), building_type.clone())
            .relationship("locatedAt", city_pisa_id.clone())
            .build()?,
    ];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(city_paris_id.clone())));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("locatedAt", any_attribute().object(city_pisa_id.clone())));
    expect_that!(entity_2, has_attribute("subCategory", any_attribute().value("religion")));
    Ok(())
}
