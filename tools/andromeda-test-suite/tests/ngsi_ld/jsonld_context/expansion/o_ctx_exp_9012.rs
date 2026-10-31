//! A context served without expiration keeps a heuristic lifetime.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::header::CACHE_CONTROL;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A remote @context served with neither Cache-Control nor Expires is still used, and not downloaded again,
/// after more than one second
/// Clause: 6.3.16, 5.13.1
/// Recorded: 6.3.16 says a broker "should" assign a heuristic expiration time when the origin server states none;
/// Andromeda assigns its configured cache lifetime, an hour by default
#[gtest]
#[tokio::test]
async fn o_ctx_exp_9012_a_context_served_without_expiration_keeps_a_heuristic_lifetime() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let document = |name: &str| {
        format!(
            r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/{name}"}}}}"#
        )
    };
    let no_directives = "ctx/no-directives.jsonld";
    let barrier = "ctx/barrier.jsonld";
    contexts
        .serve_then(
            no_directives,
            (&document("name"), ServedAnswer::ok().with_media_type("application/ld+json")),
            (&document("renamed"), ServedAnswer::ok().with_media_type("application/ld+json")),
        )
        .await?;
    let lapsing = || -> Result<ServedAnswer> {
        Ok(ServedAnswer::ok()
            .with_media_type("application/ld+json")
            .with_header(CACHE_CONTROL, "public, max-age=1".parse()?))
    };
    contexts
        .serve_then(barrier, (&document("name"), lapsing()?), (&document("renamed"), lapsing()?))
        .await?;
    for (suffix, at) in [(1, no_directives), (2, barrier)] {
        let body_context = broker
            .with_context(Context::Single(contexts.reference(at)?))?
            .with_context_delivery(ContextDelivery::LdJsonBody);
        let building = Entity::builder()
            .id(namespace.urn("Building", suffix)?)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .build()?;
        body_context.entities().create(&building).send().await?.done()?;
    }
    let first = contexts.downloads(no_directives).await?;
    expect_that!(first, ge(1));
    // The barrier context lapses after one second and was downloaded after the target, so once it is downloaded
    // again the target has been held for longer than one second.
    let barrier_linked = broker.with_context(Context::Single(contexts.reference(barrier)?))?;
    let name_iri = format!("https://test.andromeda/{namespace}/name");
    retried(async || {
        let retrieved = barrier_linked
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&namespace.urn("Building", 2)?)
            .send()
            .await?;
        verify_that!(retrieved.body(), has_attribute(&name_iri, any_attribute().value("Eiffel Tower")))
    })
    .await?;
    let target_linked = broker.with_context(Context::Single(contexts.reference(no_directives)?))?;
    let retrieved = target_linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&namespace.urn("Building", 1)?)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(contexts.downloads(no_directives).await?, eq(first));
    Ok(())
}
