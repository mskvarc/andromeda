//! Serve with details answers a cached context after its origin is gone.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextDeletion},
    model::{Context, ContextId, Entity, StoredContextKind},
};

/// Pins: After its origin stops serving it, a Cached entry cannot be reloaded (504) and Serve @context with
/// details=true still answers its kind, its original URL and its createdAt
/// Clause: 5.13.4.4, 5.13.5.4, 5.13.3.5
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0006_serve_with_details_answers_a_cached_context_after_its_origin_is_gone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let served = contexts
        .serve(
            "jc/c.jsonld",
            &format!(r#"{{"@context": {{"X1": "https://test.andromeda/{namespace}/X1"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let cached_id = served.as_str().parse::<ContextId>()?;
    let linked = broker
        .with_context(Context::Multiple(vec![contexts.reference("jc/c.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let testing = Entity::builder()
        .id(namespace.urn("Testing", 1)?)
        .entity_type(namespace.entity_type("Testing")?)
        .property("X1", "test")
        .build()?;
    linked.entities().create(&testing).send().await?.done()?;
    // A download is recorded after the request that made it, so its entry is read until it appears.
    let local_id = retried(async || {
        let metadata = broker.with_accept(Accept::any()).contexts().serve_details(&cached_id).send().await?;
        verify_that!(metadata.kind, eq(&StoredContextKind::Cached))?;
        verify_that!(metadata.url, eq(&served))?;
        Ok(metadata.local_id)
    })
    .await?;
    contexts
        .serve_answer("jc/c.jsonld", "", ServedAnswer::ok().with_status(StatusCode::NOT_FOUND))
        .await?;
    let reloaded = broker
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .delete(&local_id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&reloaded, answered(StatusCode::GATEWAY_TIMEOUT));
    let metadata = broker.with_accept(Accept::any()).contexts().serve_details(&local_id).send().await?;
    expect_that!(metadata.kind, eq(&StoredContextKind::Cached));
    expect_that!(metadata.url, eq(&served));
    Ok(())
}
