//! Serve evaluates if none match weakly with star and lists.

use andromeda_test_suite::{
    broker,
    context_terms::{defines_terms, listed_entries},
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, served_document, single_header},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Serve @context answers 304 to If-None-Match naming the weak form of the current tag, to *, and to a list
/// holding the current tag, and answers the stored document to a stale tag
/// Clause: RFC 7232 s3.2, s2.3.2
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0010_serve_evaluates_if_none_match_weakly_with_star_and_lists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let served = ServedContext::of(&broker, &hosted)?;
    let answer = served.get(&[]).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let tag = single_header(answer.as_ref().or_fail()?, &ETAG)?;
    expect_that!(tag, matches_regex(r#"^"[^"]+"$"#));
    let weak = served.get(&[(IF_NONE_MATCH, format!("W/{tag}").as_str())]).await?;
    expect_that!(&weak, answered(StatusCode::NOT_MODIFIED));
    let star = served.get(&[(IF_NONE_MATCH, "*")]).await?;
    expect_that!(&star, answered(StatusCode::NOT_MODIFIED));
    let listed = served.get(&[(IF_NONE_MATCH, format!(r#""other", {tag}"#).as_str())]).await?;
    expect_that!(&listed, answered(StatusCode::NOT_MODIFIED));
    let stale = served.get(&[(IF_NONE_MATCH, r#""stale""#)]).await?;
    expect_that!(&stale, answered(StatusCode::OK));
    let document = served_document(stale.as_ref().or_fail()?)?;
    expect_that!(listed_entries(&document.context), none());
    expect_that!(
        document.context.entries(),
        elements_are![defines_terms([("Spot", namespace.expanded_term("Spot")?.as_str())])]
    );
    Ok(())
}
