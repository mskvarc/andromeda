//! Serve a hosted context carries max age a strong tag and vary.

use andromeda_test_suite::{
    broker,
    context_terms::{defines_terms, listed_entries},
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, served_document, single_header, varies_by_tenant_and_encoding},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE, ETAG},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Serve @context answers a Hosted document exactly as stored, as application/json, with a positive max-age,
/// a strong `ETag` and a Vary naming NGSILD-Tenant
/// Clause: 5.13.4.4, 6.30.3.1, RFC 7232 s2.3, RFC 9111 s5.2.2.1
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0014_serve_a_hosted_context_carries_max_age_a_strong_tag_and_vary() -> Result<()> {
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
    let document = served_document(response)?;
    expect_that!(listed_entries(&document.context), none());
    expect_that!(
        document.context.entries(),
        elements_are![defines_terms([("Spot", namespace.expanded_term("Spot")?.as_str())])]
    );
    expect_that!(single_header(response, &CACHE_CONTROL)?, matches_regex(r"^max-age=[1-9][0-9]*$"));
    expect_that!(single_header(response, &ETAG)?, matches_regex(r#"^"[^"]+"$"#));
    Ok(())
}
