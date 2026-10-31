//! Serve head answers the headers of get without a body.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, single_header, varies_by_tenant_and_encoding},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: HEAD on a Hosted @context answers 200 with no body and the `ETag`, Cache-Control, Vary and Content-Type of
/// GET, and a conditional HEAD with the current tag answers 304
/// Clause: RFC 9110 s9.3.2, RFC 7232 s3.2
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0013_serve_head_answers_the_headers_of_get_without_a_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let served = ServedContext::of(&broker, &hosted)?;
    let answer = served.get(&[]).await?;
    expect_that!(&answer, answered(StatusCode::OK).with_header(CONTENT_TYPE, "application/json"));
    let response = answer.as_ref().or_fail()?;
    expect_that!(response.headers(), varies_by_tenant_and_encoding());
    let tag = single_header(response, &ETAG)?;
    expect_that!(tag, matches_regex(r#"^"[^"]+"$"#));
    let cache = single_header(response, &CACHE_CONTROL)?;
    let head = served.head(&[]).await?;
    expect_that!(
        &head,
        answered(StatusCode::OK)
            .without_body()
            .with_header(ETAG, &tag)
            .with_header(CACHE_CONTROL, &cache)
            .with_header(CONTENT_TYPE, "application/json")
    );
    expect_that!(head.as_ref().or_fail()?.headers(), varies_by_tenant_and_encoding());
    let conditional = served.head(&[(IF_NONE_MATCH, tag.as_str())]).await?;
    expect_that!(&conditional, answered(StatusCode::NOT_MODIFIED));
    Ok(())
}
