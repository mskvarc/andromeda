//! Serve counts each use of a cached context.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextId, Entity, StoredContextKind},
};

/// Pins: A second create naming a Cached @context raises its numberOfHits by the time the create has answered
/// Clause: 5.13.3.5, 5.13.4.4, 5.13.1
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
/// Recorded: numberOfHits is optional (clause 5.13.3.5); Andromeda reports it, and counts a use before the request
/// that made it returns
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0007_serve_counts_each_use_of_a_cached_context() -> Result<()> {
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
    let before = broker.with_accept(Accept::any()).contexts().serve_details(&local_id).send().await?;
    let hits = before.number_of_hits.or_fail()?;
    let testing_again = Entity::builder()
        .id(namespace.urn("Testing", 2)?)
        .entity_type(namespace.entity_type("Testing")?)
        .property("X1", "again")
        .build()?;
    linked.entities().create(&testing_again).send().await?.done()?;
    let after = broker.with_accept(Accept::any()).contexts().serve_details(&local_id).send().await?;
    expect_that!(after.number_of_hits, some(gt(hits)));
    Ok(())
}
