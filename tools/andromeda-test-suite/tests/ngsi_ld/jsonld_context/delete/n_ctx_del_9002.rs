//! Reload an identifier naming nothing is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::ContextDeletion,
    model::{ContextId, ErrorType},
};

/// Pins: Delete @context with reload=true of a well-formed URN naming no stored @context answers 404
/// `ResourceNotFound`
/// Clause: 5.13.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_del_9002_reload_an_identifier_naming_nothing_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unknown_id = namespace.urn_text("JsonLdContext", "unknown").parse::<ContextId>()?;
    let answer = broker
        .with_responses()
        .contexts()
        .delete(&unknown_id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
