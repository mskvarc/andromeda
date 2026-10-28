//! ETSI `002_01`: an entity is deleted by its id, after which it is no longer retrieved (clause 5.6.6).

use crate::{
    suite_keywords::create_entity,
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that one can delete an entity by id
/// Clause: 5.6.6
#[gtest]
#[tokio::test]
async fn e002_01_01_delete_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &entity).await?;
    let deleted = broker.with_responses().entities().delete(&entity.id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));
    broker.temporal().delete(&entity.id).send().await.any_answer();
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&entity.id)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    Ok(())
}
