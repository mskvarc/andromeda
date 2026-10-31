//! A retrieval under a scoped linked context is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity under a Link naming a context that defines gear with a scoped @context answers 400
/// `BadRequestData`, and under a plain one 200 with gear
/// Clause: 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9039_a_retrieval_under_a_scoped_linked_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "contexts/scoped.jsonld",
            &format!(
                r#"{{"@context": {{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/plain.jsonld",
            &format!(r#"{{"@context": {{"gear": "https://example.org/{namespace}/gear"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let device_id = namespace.urn("Device", "retrieved")?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("gear", 1).build()?;
    broker.entities().create(&device).send().await?.done()?;
    let scoped = broker.with_context(Context::Single(contexts.reference("contexts/scoped.jsonld")?))?;
    let refused = scoped
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let plain = broker.with_context(Context::Single(contexts.reference("contexts/plain.jsonld")?))?;
    let retrieved = plain
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(retrieved.body().id, eq(&device_id));
    Ok(())
}
