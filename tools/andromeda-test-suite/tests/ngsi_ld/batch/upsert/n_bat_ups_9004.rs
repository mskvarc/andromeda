//! Reversing the array reverses the outcome.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Two Batch Entity Upserts holding the same two instances of a new entity in opposite order answer 201, and
/// each entity holds the name its later instance states
/// Clause: 5.5.11.0, 5.5.11.2, 5.6.8.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_9004_reversing_the_array_reverses_the_outcome() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_forward_id = namespace.urn("Building", "forward")?;
    let building_type = namespace.entity_type("Building")?;
    let building_reversed_id = namespace.urn("Building", "reversed")?;
    let entities = [
        Entity::builder()
            .id(building_forward_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(building_forward_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Tour Eiffel")
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let entities_2 = [
        Entity::builder()
            .id(building_reversed_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Tour Eiffel")
            .build()?,
        Entity::builder()
            .id(building_reversed_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?,
    ];
    broker.entities().batch_upsert(&entities_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_forward_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Tour Eiffel")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_reversed_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
