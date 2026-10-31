//! Minimal entities answer 201 with their ids and read back.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, OneOrMany},
};

/// Pins: a Batch Entity Creation of two entities carrying only id and type answers 201 with an array of exactly
/// both ids, and each entity then reads back
/// Clause: 5.6.7.4, 5.6.7.5, 6.14.3.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_bat_cre_0001_minimal_entities_answer_201_with_their_ids_and_read_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
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
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_2_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type.clone())));
    Ok(())
}
