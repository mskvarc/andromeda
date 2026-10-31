//! Deleting a `@context` the broker does not store is refused with 404 `ResourceNotFound` (clause 5.13.5).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that an error message is obtained in the response when one tries to delete a @context with unknonwn id
/// Clause: 5.13.5
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_02_01_delete_a_context_with_unknown_context_identifier() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // Hand-written: the identifier is the bare namespace token, no URI, which a typed identifier refuses.
    let request = wire::request_to(&broker, Method::DELETE, &format!("jsonldContexts/{}", namespace.token()))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_distinct_detail());
    Ok(())
}
