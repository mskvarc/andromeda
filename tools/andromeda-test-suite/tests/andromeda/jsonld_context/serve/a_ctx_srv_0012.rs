//! Serve a compressed document carries a weak tag that revalidates.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, single_header},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{ACCEPT_ENCODING, CONTENT_ENCODING, ETAG, IF_NONE_MATCH},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Serve @context of a large document compressed with gzip carries the weak form of the identity
/// representation's strong tag, and If-None-Match with that weak tag answers 304
/// Clause: RFC 7232 s2.3.3, s3.2
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0012_serve_a_compressed_document_carries_a_weak_tag_that_revalidates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let terms = (0..200)
        .map(|index| format!(r#""term{index}": "https://test.andromeda/{namespace}/term{index}""#))
        .collect::<Vec<_>>()
        .join(", ");
    let context = Context::Single(ContextEntry::Inline(format!("{{{terms}}}").parse::<InlineContext>()?));
    let hosted = HostedContext::add(&broker, &context).await?;
    let served = ServedContext::of(&broker, &hosted)?;
    let identity = served.get(&[(ACCEPT_ENCODING, "identity")]).await?;
    expect_that!(&identity, answered(StatusCode::OK).without_header(CONTENT_ENCODING));
    let tag = single_header(identity.as_ref().or_fail()?, &ETAG)?;
    expect_that!(tag, matches_regex(r#"^"[^"]+"$"#));
    let gzip = served.get(&[(ACCEPT_ENCODING, "gzip")]).await?;
    let weak = format!("W/{tag}");
    expect_that!(&gzip, answered(StatusCode::OK).with_header(CONTENT_ENCODING, "gzip").with_header(ETAG, &weak));
    let revalidated = served.get(&[(ACCEPT_ENCODING, "gzip"), (IF_NONE_MATCH, weak.as_str())]).await?;
    expect_that!(&revalidated, answered(StatusCode::NOT_MODIFIED));
    Ok(())
}
