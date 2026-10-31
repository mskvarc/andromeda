//! Serve the entity tag follows the document content.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, single_header},
};
use googletest::prelude::*;
use http::{StatusCode, header::ETAG};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Serve @context answers the same `ETag` for the same document, served again or stored under another
/// identifier, and a different `ETag` for a different document
/// Clause: 5.13.4.4, RFC 7232 s2.3
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0016_serve_the_entity_tag_follows_the_document_content() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let first = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let changed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot2"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let twin = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let first_served = ServedContext::of(&broker, &first)?;
    let answer = first_served.get(&[]).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let tag = single_header(answer.as_ref().or_fail()?, &ETAG)?;
    expect_that!(tag, matches_regex(r#"^"[^"]+"$"#));
    let again = first_served.get(&[]).await?;
    expect_that!(&again, answered(StatusCode::OK).with_header(ETAG, &tag));
    let changed_answer = ServedContext::of(&broker, &changed)?.get(&[]).await?;
    expect_that!(&changed_answer, answered(StatusCode::OK));
    let changed_tag = single_header(changed_answer.as_ref().or_fail()?, &ETAG)?;
    expect_that!(changed_tag, matches_regex(r#"^"[^"]+"$"#));
    expect_that!(changed_tag, not(eq(&tag)));
    let twin_answer = ServedContext::of(&broker, &twin)?.get(&[]).await?;
    expect_that!(&twin_answer, answered(StatusCode::OK).with_header(ETAG, &tag));
    Ok(())
}
