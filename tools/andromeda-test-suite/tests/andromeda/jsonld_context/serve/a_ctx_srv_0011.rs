//! Serve a precondition does not mask 404 or 422.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    remote_context::RemoteContexts,
    served_context::ServedContext,
};
use googletest::prelude::*;
use http::{StatusCode, header::IF_NONE_MATCH};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextId, Entity, ErrorType, StoredContextKind},
};

/// Pins: Serve @context with If-None-Match: * still answers 404 `ResourceNotFound` for an unknown identifier and
/// 422 `OperationNotSupported` for a Cached entry
/// Clause: RFC 7232 s5, 5.13.4.4
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0011_serve_a_precondition_does_not_mask_404_or_422() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let served_url = contexts
        .serve(
            "jc/c.jsonld",
            &format!(r#"{{"@context": {{"X1": "https://test.andromeda/{namespace}/X1"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
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
    let cached_id = served_url.as_str().parse::<ContextId>()?;
    let local_id = retried(async || {
        let metadata = broker.with_accept(Accept::any()).contexts().serve_details(&cached_id).send().await?;
        verify_that!(metadata.kind, eq(&StoredContextKind::Cached))?;
        verify_that!(metadata.url, eq(&served_url))?;
        Ok(metadata.local_id)
    })
    .await?;
    let unknown = ServedContext::at(&broker, &namespace.urn_text("JsonLdContext", "unknown"));
    let answer = unknown.get(&[(IF_NONE_MATCH, "*")]).await?;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let cached = ServedContext::at(&broker, &local_id.to_string());
    let answer_2 = cached.get(&[(IF_NONE_MATCH, "*")]).await?;
    expect_that!(&answer_2, problem(StatusCode::UNPROCESSABLE_ENTITY, ErrorType::OperationNotSupported));
    Ok(())
}
