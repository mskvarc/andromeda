//! Serve the core context url is resource not found.

use andromeda_test_suite::{broker, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextId, ErrorType},
};

/// Pins: Serve @context of the core @context, content or details, answers 404 `ResourceNotFound`
/// Clause: 5.13.4.4
/// Recorded: clause 5.13.4.4 gives back `ResourceNotFound` for an identifier that is not found; the core @context
/// is not a stored entry, so its URL is one
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0018_serve_the_core_context_url_is_resource_not_found() -> Result<()> {
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve_details(&"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
