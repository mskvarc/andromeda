//! Serve a lowercased kind names no stored entry.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::problem, wire};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ServeContext},
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Serve @context with the identifier's namespace-specific string lowercased (jsonldcontext) answers 404
/// `ResourceNotFound`
/// Clause: 5.13.4.4, RFC 8141 s3.1
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0026_serve_a_lowercased_kind_names_no_stored_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    broker.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    let (_, tail) = ctx.url.path().rsplit_once("/jsonldContexts/").or_fail()?;
    let lower = tail.replacen("JsonLdContext", "jsonldcontext", 1);
    expect_that!(lower, contains_substring("jsonldcontext"));
    // The identifier is sent in the spelling the test builds, which the typed operation would not write.
    let answer = wire::send_to(
        &broker.with_accept(Accept::any()),
        &ServeContext::builder().id(&ctx.id).build(),
        &format!("jsonldContexts/{lower}"),
    )
    .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
