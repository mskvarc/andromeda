//! Serve an uppercase urn prefix addresses the stored entry.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, wire};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ServeContext},
    model::{Context, ContextEntry, InlineContext, LocalContext},
};
use std::str::from_utf8;

/// Pins: Serve @context with the identifier spelled with an uppercase urn scheme and namespace identifier answers
/// the document stored under the lowercase spelling
/// Clause: 5.13.4.4, RFC 8141 s3.1
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0025_serve_an_uppercase_urn_prefix_addresses_the_stored_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    broker.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    let (_, tail) = ctx.url.path().rsplit_once("/jsonldContexts/").or_fail()?;
    let upper = tail.replacen("urn:ngsi-ld", "URN:NGSI-LD", 1);
    expect_that!(upper, starts_with("URN:NGSI-LD"));
    // The identifier is sent in the spelling the test builds, which the typed operation would not write.
    let answer = wire::send_to(
        &broker.with_accept(Accept::any()),
        &ServeContext::builder().id(&ctx.id).build(),
        &format!("jsonldContexts/{upper}"),
    )
    .await?;
    expect_that!(answer.status().as_u16(), eq(200));
    expect_that!(LocalContext::parse(from_utf8(answer.body())?)?.context, eq(&context));
    Ok(())
}
