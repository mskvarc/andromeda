//! ETSI `018_02`: an entity is not retrieved by an identifier that is not a URI (clause 5.7.1).

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::{APPLICATION_LD_JSON, ErrorType};

/// Pins: Check that one cannot get an entity with invalid/missing id
/// Clause: 5.7.1
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e018_02_01_get_an_entity_if_the_entity_id_is_not_a_valid_uri() -> Result<()> {
    let client = broker::front()?;
    let request =
        request_to(&client, Method::GET, "entities/thisisaninvaliduri")?.append_header(ACCEPT, HeaderValue::try_from(APPLICATION_LD_JSON.to_string())?);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title())
}
