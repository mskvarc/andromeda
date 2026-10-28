//! ETSI `051_02`: deleting a `@context` the broker does not store is refused with 404 `ResourceNotFound` (clause
//! 5.13.5).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that an error message is obtained in the response when one tries to delete a @context with unknonwn id
/// Clause: 5.13.5
/// Changed: the random 16-digit identifier is {{ns}}, unique per file and per run as the random digits are
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
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
