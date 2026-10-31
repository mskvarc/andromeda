//! Reload a cached context whose origin is gone is ld context not available and keeps it.

use andromeda_test_suite::{
    broker,
    context_record,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, operation::ContextDeletion},
    model::{Context, Entity, ErrorType, StoredContextKind},
};

/// Pins: Delete @context with reload=true of a Cached entry whose origin no longer serves it answers 504
/// `LdContextNotAvailable`, and the entry is still served as Cached
/// Clause: 5.13.5.4, 6.30.3.2, 5.5.2
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_del_0006_reload_a_cached_context_whose_origin_is_gone_is_ld_context_not_available_and_keeps_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let document = format!(r#"{{"@context": {{"X1": "https://test.andromeda/{namespace}/X1"}}}}"#);
    contexts.serve("jc/c.jsonld", &document, Some("application/ld+json")).await?;
    let testing = Entity::builder()
        .id(namespace.urn("Testing", 1)?)
        .entity_type(namespace.entity_type("Testing")?)
        .property("X1", "test")
        .build()?;
    broker
        .with_context(Context::Multiple(vec![contexts.reference("jc/c.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&testing)
        .send()
        .await?
        .done()?;
    let url = contexts.url("jc/c.jsonld")?;
    // A download is recorded after the request that made it, so its entry is read until it appears.
    context_record::cached_under(&broker, &url).await?;
    let record = context_record::stored(&broker, &context_record::segment_of(&url)).await?;
    // The origin stops serving the document: a path with no document answers 404.
    contexts
        .serve_answer("jc/c.jsonld", &document, ServedAnswer::ok().with_status(StatusCode::NOT_FOUND))
        .await?;
    let reloaded = broker
        .with_responses()
        .contexts()
        .delete(&record.local_id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&reloaded, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let kept = context_record::stored(&broker, &record.local_id.to_string()).await?;
    expect_that!(kept.kind, eq(&StoredContextKind::Cached));
    Ok(())
}
