//! A context source registration retrieval whose id is not a URI is refused with 400 (clause 5.10.1).

use crate::unchecked_requests::refused;
use andromeda_test_suite::{broker, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot retrieve a Context Source Registration, if the context source registration id is not a
/// valid URI
/// Clause: 5.10.1
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e036_01_01_invalid_id() -> Result<()> {
    let broker = broker::front()?;
    // No typed registration id is `invalidUri`.
    let request = request_to(&broker, Method::GET, "csourceRegistrations/invalidUri")?.append_header(ACCEPT, HeaderValue::from_static("*/*"));
    refused(&broker, request, ErrorType::BadRequestData).await
}
