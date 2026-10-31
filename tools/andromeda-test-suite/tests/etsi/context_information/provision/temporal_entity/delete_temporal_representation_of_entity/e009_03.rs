//! A temporal representation of an entity is not deleted by an id the broker does not hold (clause 5.6.16).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Check that an error is raised if one deletes a temporal entity with a non-existing entity id
/// Clause: 5.6.16
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e009_03_01_delete_a_temporal_representation_of_an_entity_with_an_unknown_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Vehicle", 1)?;
    let deleted = broker.with_responses().temporal().delete(&id).send().await;
    expect_that!(&deleted, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
