//! A cached context expires with its s maxage over a longer max age.

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

/// Pins: A remote @context served with Cache-Control max-age=3600 and s-maxage=1 is downloaded again once one second
/// has passed
/// Clause: 6.3.16, RFC 7234 s5.2.2.9
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9005_a_cached_context_expires_with_its_s_maxage_over_a_longer_max_age() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let path = "ctx/s-maxage.jsonld";
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
                .with_header(CACHE_CONTROL, "public, max-age=3600, s-maxage=1".parse()?),
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
    // The entry lapses after its one-second s-maxage; the next use downloads the renamed document, so the name is no
    // longer compacted.
    contexts
        .serve_answer(
            path,
            &document("renamed"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=3600, s-maxage=1".parse()?),
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
