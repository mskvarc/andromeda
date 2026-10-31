//! A not modified answer carries every vary of the 200.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, single_header, varies_by_tenant_and_encoding},
    wire::header_values,
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{ETAG, IF_NONE_MATCH, VARY},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Serve @context with If-None-Match naming the current entity tag, sent without Accept-Encoding, answers 304
/// with every Vary field line the 200 to the same request carries
/// Clause: RFC 7232 s4.1
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0023_a_not_modified_answer_carries_every_vary_of_the_200() -> Result<()> {
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
    let not_modified = served.get(&[(IF_NONE_MATCH, tag.as_str())]).await?;
    expect_that!(&not_modified, answered(StatusCode::NOT_MODIFIED));
    let not_modified_response = not_modified.as_ref().or_fail()?;
    expect_that!(not_modified_response.headers(), varies_by_tenant_and_encoding());
    expect_that!(
        header_values(not_modified_response.headers(), &VARY),
        eq(&header_values(response.headers(), &VARY))
    );
    Ok(())
}
