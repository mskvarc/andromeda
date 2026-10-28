//! ETSI `002_02`: an entity cannot be deleted by an id that is not a valid URI (clause 5.6.6).

use crate::suite_keywords::refused;
use andromeda_test_suite::{broker, teardown::AnyAnswer, wire::request_to};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot delete an entity with invalid id
/// Clause: 5.6.6
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e002_02_01_entity_id_not_a_valid_uri() -> Result<()> {
    let broker = broker::front()?;
    refused(
        &broker,
        request_to(&broker, Method::DELETE, "entities/thisisaninvaliduri")?,
        ErrorType::BadRequestData,
    )
    .await?;
    broker
        .send_unchecked(request_to(&broker, Method::DELETE, "temporal/entities/thisisaninvaliduri")?)
        .await
        .any_answer();
    Ok(())
}
