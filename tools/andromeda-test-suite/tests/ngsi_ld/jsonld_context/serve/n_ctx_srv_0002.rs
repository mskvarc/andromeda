//! Serve an unknown identifier is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextId, ErrorType},
};

/// Pins: Serve @context of an identifier the broker does not hold answers 404 `ResourceNotFound`
/// Clause: 5.13.4.4, 6.30.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0002_serve_an_unknown_identifier_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&format!("urn:ngsi-ld:unknown:{namespace}-ctx").parse::<ContextId>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
