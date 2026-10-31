//! A stored identifier addresses its entry until it is deleted.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, ErrorType, InlineContext, StoredContextKind},
};

/// Pins: The identifier Add @context answers with serves the stored document, reports kind Hosted and that
/// identifier as `localId`, and is deleted with 204, after which serving it answers 404 `ResourceNotFound`
/// Clause: 5.13.2.5, 5.13.3.5, 5.13.4.4, 5.13.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_del_9004_a_stored_identifier_addresses_its_entry_until_it_is_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"A1": "urn:ngsi-ld:attributes:{namespace}-A1"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let served = broker.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    expect_that!(served, eq(&context));
    let answer = context_details::serve_details(&broker, &ctx.id).await?;
    let metadata = context_details::metadata(&answer)?;
    expect_that!(metadata.kind, eq(&StoredContextKind::Hosted));
    expect_that!(metadata.local_id, eq(&ctx.id));
    broker.contexts().delete(&ctx.id).send().await?;
    let gone = broker.with_responses().with_accept(Accept::any()).contexts().serve(&ctx.id).send().await;
    expect_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
