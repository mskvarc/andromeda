//! Serve with details carries no validators.

use andromeda_test_suite::{
    broker,
    context_details::metadata,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    served_context::ServedContext,
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CACHE_CONTROL, ETAG},
};
use ngsi_ld::model::{Context, ContextEntry, InlineContext, StoredContextKind};

/// Pins: Serve @context with details=true answers the metadata without an `ETag` or a Cache-Control
/// Clause: 5.13.4.4, 5.13.3.5
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0017_serve_with_details_carries_no_validators() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let details = ServedContext::of(&broker, &hosted)?.details().await?;
    expect_that!(&details, answered(StatusCode::OK).without_header(ETAG).without_header(CACHE_CONTROL));
    expect_that!(metadata(details.as_ref().or_fail()?)?.kind, eq(&StoredContextKind::Hosted));
    Ok(())
}
