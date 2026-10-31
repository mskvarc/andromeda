//! An id list deletes the listed entities only.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Purge Entities with a list of ids and local=true answers 204, deletes the listed entities and keeps an
/// unlisted entity of the same type
/// Clause: 5.6.21.4, 6.4.3.3, 5.5.13
#[gtest]
#[tokio::test]
async fn n_ent_prg_0002_an_id_list_deletes_the_listed_entities_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_3_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    broker
        .entities()
        .purge()
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_3_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_3_id));
    Ok(())
}
