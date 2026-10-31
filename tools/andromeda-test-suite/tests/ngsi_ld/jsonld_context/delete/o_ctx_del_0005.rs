//! Delete a cached context answers 204.

use andromeda_test_suite::{broker, context_record, namespace::Namespace, outcome::Done, problem::answered, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity},
};

/// Pins: Delete @context of a Cached entry, by its local identifier, answers 204 with no body
/// Clause: 5.13.5.4, 6.30.3.2, 5.13.1
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_del_0005_delete_a_cached_context_answers_204() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "jc/c.jsonld",
            &format!(r#"{{"@context": {{"X1": "https://test.andromeda/{namespace}/X1"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
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
    let deleted = broker.with_responses().contexts().delete(&record.local_id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT).without_body());
    Ok(())
}
