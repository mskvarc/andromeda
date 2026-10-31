//! A lapsed cached context is invalidated without another use.

use andromeda_test_suite::{
    broker,
    context_record,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::header::CACHE_CONTROL;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, StoredContextKind},
};

/// Pins: A remote @context served with Cache-Control max-age=3 and named once is listed as Cached, and its entry
/// disappears without any further request naming it
/// Clause: 5.13.1, 6.3.16, 5.13.4.4
#[gtest]
#[tokio::test]
async fn n_ctx_srv_9001_a_lapsed_cached_context_is_invalidated_without_another_use() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve_answer(
            "ctx/invalidated.jsonld",
            &format!(
                r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}}}"#
            ),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=3".parse()?),
        )
        .await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(namespace.entity_type("Building")?)
        .property("name", "Eiffel Tower")
        .build()?;
    broker
        .with_context(Context::Single(contexts.reference("ctx/invalidated.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&building)
        .send()
        .await?
        .done()?;
    let url = contexts.url("ctx/invalidated.jsonld")?;
    // A download is recorded after the request that made it, so its entry is read until it appears, well inside its
    // three seconds.
    context_record::cached_under(&broker, &url).await?;
    let record = context_record::stored(&broker, &context_record::segment_of(&url)).await?;
    expect_that!(record.kind, eq(&StoredContextKind::Cached));
    expect_that!(record.url, eq(&url));
    // Nothing names the @context again, so the entry has to go because the periodic invalidation came round.
    retried(async || context_record::unrecorded(&broker, &url).await).await?;
    expect_that!(contexts.downloads("ctx/invalidated.jsonld").await?, eq(1));
    Ok(())
}
