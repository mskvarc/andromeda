//! Serve with the current tag answers not modified and counts the use.

use andromeda_test_suite::{
    broker,
    context_details::metadata,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, single_header, varies_by_tenant_and_encoding},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Serve @context with If-None-Match naming the current entity tag answers 304 with no body and the `ETag`
/// and Cache-Control of the 200, and the access raises `numberOfHits`
/// Clause: RFC 7232 s4.1, s3.2, 5.13.3.5
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0009_serve_with_the_current_tag_answers_not_modified_and_counts_the_use() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let served = ServedContext::of(&broker, &hosted)?;
    let answer = served.get(&[]).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let response = answer.as_ref().or_fail()?;
    expect_that!(response.headers(), varies_by_tenant_and_encoding());
    let tag = single_header(response, &ETAG)?;
    expect_that!(tag, matches_regex(r#"^"[^"]+"$"#));
    let cache = single_header(response, &CACHE_CONTROL)?;
    let details = served.details().await?;
    expect_that!(&details, answered(StatusCode::OK));
    let hits = metadata(details.as_ref().or_fail()?)?.number_of_hits.or_fail()?;
    let not_modified = served.get(&[(IF_NONE_MATCH, tag.as_str())]).await?;
    expect_that!(
        &not_modified,
        answered(StatusCode::NOT_MODIFIED)
            .without_body()
            .with_header(ETAG, &tag)
            .with_header(CACHE_CONTROL, &cache)
    );
    let details_2 = served.details().await?;
    expect_that!(&details_2, answered(StatusCode::OK));
    expect_that!(metadata(details_2.as_ref().or_fail()?)?.number_of_hits, some(gt(hits)));
    Ok(())
}
