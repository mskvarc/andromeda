//! Serve a cached context is operation not supported and its details are served.

use andromeda_test_suite::{broker, context_details, context_record, namespace::Namespace, outcome::Done, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, ErrorType, StoredContextKind},
};

/// Pins: Serve @context without details of a Cached @context answers 422 `OperationNotSupported`, and with
/// details=true answers 200
/// Clause: 5.13.4.4, 5.13.1
#[gtest]
#[tokio::test]
async fn n_ctx_srv_9002_serve_a_cached_context_is_operation_not_supported_and_its_details_are_served() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "ctx/not-served.jsonld",
            &format!(
                r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(namespace.entity_type("Building")?)
        .property("name", "Eiffel Tower")
        .build()?;
    broker
        .with_context(Context::Single(contexts.reference("ctx/not-served.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&building)
        .send()
        .await?
        .done()?;
    let url = contexts.url("ctx/not-served.jsonld")?;
    // A download is recorded after the request that made it, so its entry is read until it appears.
    context_record::cached_under(&broker, &url).await?;
    let record = context_record::stored(&broker, &context_record::segment_of(&url)).await?;
    expect_that!(record.kind, eq(&StoredContextKind::Cached));
    let served = broker
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&record.local_id)
        .send()
        .await;
    expect_that!(&served, problem(StatusCode::UNPROCESSABLE_ENTITY, ErrorType::OperationNotSupported));
    let answer = context_details::serve_details(&broker, &record.local_id).await?;
    expect_that!(context_details::metadata(&answer)?.kind, eq(&StoredContextKind::Cached));
    Ok(())
}
