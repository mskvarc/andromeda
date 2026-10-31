//! An entity cannot be deleted by an id the broker does not hold (clause 5.6.6).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot delete an entity if the entity id is not known to the system
/// Clause: 5.6.6
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e002_03_01_entity_id_not_known() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Building", 1)?;
    let deleted = broker.with_responses().entities().delete(&id).send().await;
    expect_that!(&deleted, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
