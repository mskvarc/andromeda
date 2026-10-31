//! Id takes precedence over id pattern in the entities purged.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{IdPattern, Locality},
};

/// Pins: Purge Entities that gives both id and `idPattern` deletes the entity the id names and none the pattern
/// alone would select
/// Clause: 5.2.33, 5.6.21.4, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_3001_id_takes_precedence_over_id_pattern_in_the_entities_purged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_a1_id = namespace.urn("Building", "a1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a2_id = namespace.urn("Building", "a2")?;
    let building_b1_id = namespace.urn("Building", "b1")?;
    let entities = [
        Entity::builder().id(building_a1_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_a2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_b1_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 5.2.33: "id takes precedence over idPattern": b1 is named, and the pattern matches only the a entities
    broker
        .entities()
        .purge()
        .entity_type(&building_type)
        .ids(vec![building_b1_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_b1_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_a1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_a1_id));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_a2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_a2_id));
    Ok(())
}
