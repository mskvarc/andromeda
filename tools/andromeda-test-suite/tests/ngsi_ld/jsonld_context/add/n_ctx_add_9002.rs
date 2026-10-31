//! A hosted context may declare a scoped context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
};

/// Pins: Add @context of a document defining gear with a scoped @context answers 201, the hosted @context is served
/// with the scoped definition as written, and Delete @context answers 204
/// Clause: 5.13.2, 5.13.4, 5.13.5
#[gtest]
#[tokio::test]
async fn n_ctx_add_9002_a_hosted_context_may_declare_a_scoped_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let written =
        format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#);
    let context = Context::Single(ContextEntry::Inline(written.parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let served = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .contexts()
        .serve(&ctx.id)
        .send()
        .await?;
    let Context::Single(ContextEntry::Inline(terms)) = served else {
        return fail!("the served context is not one inline object: {served:?}");
    };
    expect_that!(
        terms.to_string(),
        eq(&format!(
            r#"{{"gear":{{"@id":"https://example.org/{namespace}/gear","@context":{{"observedAt":"https://example.org/{namespace}/rewritten"}}}}}}"#
        ))
    );
    let deleted = broker.with_responses().contexts().delete(&ctx.id).send().await?;
    expect_that!(deleted.body.len(), eq(0));
    Ok(())
}
