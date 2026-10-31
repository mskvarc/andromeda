//! A delete of an unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Delete Entity of an identifier never created answers 404 `ResourceNotFound`
/// Clause: 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_ent_del_9004_a_delete_of_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_never_created_id = namespace.urn("Building", "never-created")?;
    let answer = broker.with_responses().entities().delete(&building_never_created_id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
