//! ETSI `020_06`: the temporal evolution of an entity whose identifier is not a URI is refused with 400
//! `BadRequestData` (clause 5.7.3).

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot retrieve the temporal evolution of an entity with an invalid id (invalid URI)
/// Clause: 5.7.3
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e020_06_01_retrieve_the_temporal_evolution_of_an_entity_with_an_invalid_id() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::GET, "temporal/entities/invalidUri")?;
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title())
}
