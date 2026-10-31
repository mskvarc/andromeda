//! A cached context is addressable by its local identifier or its original url.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextId, Entity, StoredContextKind},
};

/// Pins: Serve @context with details of a Cached entry answers the same localId and URL whether addressed by its
/// version 4 local identifier or by the URL it was downloaded from
/// Clause: 5.13.4.3, 6.30.2, 5.13.1
/// Recorded: clause 5.13.1 leaves the format of a stored @context's local identifier to the broker; Andromeda names
/// every entry urn:ngsi-ld:JsonLdContext:<uuid>, a random (version 4) UUID for a Hosted or Cached entry and the
/// version 5 UUID of the canonical content for an `ImplicitlyCreated` one
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0024_a_cached_context_is_addressable_by_its_local_identifier_or_its_original_url() -> Result<()> {
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
    let metadata = broker.with_accept(Accept::any()).contexts().serve_details(&local_id).send().await?;
    expect_that!(metadata.kind, eq(&StoredContextKind::Cached));
    expect_that!(metadata.local_id, eq(&local_id));
    expect_that!(
        metadata.local_id.to_string(),
        matches_regex("^urn:ngsi-ld:JsonLdContext:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    );
    expect_that!(metadata.url, eq(&served));
    Ok(())
}
