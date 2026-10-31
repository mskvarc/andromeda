//! Delete a hosted context answers 204 and removes it.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext},
};

/// Pins: Delete @context of a Hosted entry answers 204 with no body, and Serve @context then answers 404
/// Clause: 5.13.5.4, 6.30.3.2, 5.13.4.4
#[gtest]
#[tokio::test]
async fn n_ctx_del_0001_delete_a_hosted_context_answers_204_and_removes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"A1": "https://test.andromeda/{namespace}/A1", "A2": "https://test.andromeda/{namespace}/A2", "A3": "https://test.andromeda/{namespace}/A3"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let received = broker.with_responses().contexts().delete(&ctx.id).send().await?;
    expect_that!(received.body.len(), eq(0));
    let answer = broker.with_responses().with_accept(Accept::any()).contexts().serve(&ctx.id).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
