//! Reload of a hosted context is bad request data and keeps it.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextDeletion},
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Delete @context with reload true of a Hosted @context answers 400 `BadRequestData`, and the @context is
/// still served
/// Clause: 5.13.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_del_9005_reload_of_a_hosted_context_is_bad_request_data_and_keeps_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"A1": "urn:ngsi-ld:attributes:{namespace}-A1"}}"#).parse::<InlineContext>()?,
    ));
    let ctx_id = HostedContext::add(&broker, &context).await?;
    let answer = broker
        .with_responses()
        .contexts()
        .delete(&ctx_id.id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker.with_accept(Accept::any()).contexts().serve(&ctx_id.id).send().await?;
    Ok(())
}
