//! A hosted context is served with its members in written order.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext},
};

/// Pins: Serve @context answers a hosted @context with its term definitions in the order Add @context wrote them,
/// the longer term first
/// Clause: 5.13.2.4, 5.13.4.4
/// Recorded: clause 5.13.4.4 serves "the full content of the @context" and IETF RFC 8259 section 4 makes a JSON
/// object an unordered collection, so the member order served is the broker's choice; Andromeda keeps the document
/// as it was written
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0027_a_hosted_context_is_served_with_its_members_in_written_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let written =
        format!(r#"{{"zulu_member_{namespace}": "https://example.org/{namespace}/zulu", "alpha_{namespace}": "https://example.org/{namespace}/alpha"}}"#);
    let context = Context::Single(ContextEntry::Inline(written.parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    // A document normalised by its keys would put the shorter term first, so the written order is told apart from a
    // normalised one; the compact text of the served object holds its two terms and nothing else.
    let served = broker.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    let Context::Single(ContextEntry::Inline(terms)) = served else {
        return fail!("the served context is not one inline object: {served:?}");
    };
    expect_that!(
        terms.to_string(),
        eq(&format!(
            r#"{{"zulu_member_{namespace}":"https://example.org/{namespace}/zulu","alpha_{namespace}":"https://example.org/{namespace}/alpha"}}"#
        ))
    );
    Ok(())
}
