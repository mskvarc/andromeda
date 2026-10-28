//! ETSI `009_02`: a temporal representation of an entity is not deleted by a request without an entity id, nor by one whose
//! id the broker refuses (clause 5.6.16).

use crate::suite_keywords::refused;
use andromeda_test_suite::{broker, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that an error is raised if one deletes a temporal entity with an empty/invalid `EntityId`
/// Clause: 5.6.16
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e009_02_01_delete_a_temporal_representation_of_an_entity_with_an_empty_entity_id() -> Result<()> {
    let broker = broker::front()?;
    let answer = broker.send_unchecked(request_to(&broker, Method::DELETE, "temporal/entities/")?).await;
    expect_that!(&answer, answered(StatusCode::METHOD_NOT_ALLOWED));
    Ok(())
}

/// Pins: Check that an error is raised if one deletes a temporal entity with an empty/invalid `EntityId`
/// Clause: 5.6.16
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e009_02_02_delete_a_temporal_representation_of_an_entity_with_an_invalid_entity_id() -> Result<()> {
    let broker = broker::front()?;
    refused(
        &broker,
        request_to(&broker, Method::DELETE, "temporal/entities/invalidId")?,
        ErrorType::BadRequestData,
    )
    .await
}
