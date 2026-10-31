//! A cached context expires with its max age.

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

/// Pins: A remote @context served with Cache-Control max-age=1 is downloaded again once that second has passed, not
/// after a configured lifetime
/// Clause: 6.3.16, 5.13.1, RFC 7234 s5.2.2.8
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9004_a_cached_context_expires_with_its_max_age() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let path = "ctx/max-age.jsonld";
    let building_type = namespace.entity_type("Building")?;
    let document = |name: &str| {
        format!(
            r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/{name}"}}}}"#
        )
    };
    contexts
        .serve_answer(
            path,
            &document("name"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=1".parse()?),
        )
        .await?;
    let body_context = broker
        .with_context(Context::Single(contexts.reference(path)?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building_id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    body_context.entities().create(&building).send().await?.done()?;
    let first = contexts.downloads(path).await?;
    expect_that!(first, ge(1));
    // The entry lapses after its one-second lifetime; the next use downloads the renamed document, so the name is no
    // longer compacted.
    contexts
        .serve_answer(
            path,
            &document("renamed"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=1".parse()?),
        )
        .await?;
    let linked = broker.with_context(Context::Single(contexts.reference(path)?))?;
    let name_iri = format!("https://test.andromeda/{namespace}/name");
    retried(async || {
        let retrieved = linked
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&building_id)
            .send()
            .await?;
        verify_that!(retrieved.body(), has_attribute(&name_iri, any_attribute().value("Eiffel Tower")))?;
        Ok(())
    })
    .await?;
    expect_that!(contexts.downloads(path).await?, gt(first));
    Ok(())
}
