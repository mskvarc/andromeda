//! An id pattern deletes the matching entities only.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{IdPattern, Locality},
};

/// Pins: Purge Entities with an `idPattern` and local=true answers 204, deletes the entities whose id matches and
/// keeps one whose id does not
/// Clause: 5.6.21.4, 6.4.3.3, 5.5.13
#[gtest]
#[tokio::test]
async fn n_ent_prg_0003_an_id_pattern_deletes_the_matching_entities_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_match1_id = namespace.urn("Building", "match1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_match2_id = namespace.urn("Building", "match2")?;
    let building_other_id = namespace.urn("Building", "other")?;
    let entities = [
        Entity::builder().id(building_match1_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_match2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_other_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    broker
        .entities()
        .purge()
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-match.*").parse::<IdPattern>()?)
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_match1_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_match2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_other_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_other_id));
    Ok(())
}
