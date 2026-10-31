//! A delete whose target is not a valid URI is refused with 400 `BadRequestData` (clause 5.11.6).

use crate::unchecked_requests::refused;
use andromeda_test_suite::{broker, wire::request_to};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot delete a context source registration subscription with an invalid URI
/// Clause: 5.11.6
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e042_02_01_delete_context_source_registration_subscription_with_invalid_uri() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::DELETE, "csourceSubscriptions/invalidUri")?;
    refused(&broker, request, ErrorType::BadRequestData).await
}
