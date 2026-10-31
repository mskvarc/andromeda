//! A cached context keeps a max age over a conflicting expires.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use chrono::{Duration, Utc};
use googletest::prelude::*;
use http::header::{CACHE_CONTROL, EXPIRES};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A remote @context served with Cache-Control max-age=3600 and an Expires header one second out is still used,
/// and not downloaded again, after more than one second, because max-age is consulted before Expires
/// Clause: 6.3.16, RFC 7234 s4.2.1, RFC 7234 s5.3
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9008_a_cached_context_keeps_a_max_age_over_a_conflicting_expires() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let path = "ctx/conflicting-expires.jsonld";
    let barrier_path = "ctx/barrier.jsonld";
    let building_type = namespace.entity_type("Building")?;
    let document = |name: &str| {
        format!(
            r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/{name}"}}}}"#
        )
    };
    let expires = |seconds: i64| -> Result<String> {
        // Two seconds or one second past now: the addition cannot overflow.
        let instant = Utc::now() + Duration::seconds(seconds);
        Ok(instant.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
    };
    contexts
        .serve_answer(
            path,
            &document("name"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=3600".parse()?)
                .with_header(EXPIRES, expires(1)?.parse()?),
        )
        .await?;
    contexts
        .serve_answer(
            barrier_path,
            &document("name"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=1".parse()?),
        )
        .await?;
    let building_id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker
        .with_context(Context::Single(contexts.reference(path)?))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&building)
        .send()
        .await?
        .done()?;
    let barrier_id = namespace.urn("Building", 2)?;
    let barrier_building = Entity::builder()
        .id(barrier_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .build()?;
    broker
        .with_context(Context::Single(contexts.reference(barrier_path)?))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&barrier_building)
        .send()
        .await?
        .done()?;
    let first = contexts.downloads(path).await?;
    expect_that!(first, ge(1));
    // The barrier @context lapses after one second and was downloaded after the target, so once it is downloaded
    // again the target has been held for longer than one second.
    contexts
        .serve_answer(
            barrier_path,
            &document("renamed"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "public, max-age=1".parse()?),
        )
        .await?;
    let barrier_linked = broker.with_context(Context::Single(contexts.reference(barrier_path)?))?;
    let name_iri = format!("https://test.andromeda/{namespace}/name");
    retried(async || {
        let retrieved = barrier_linked
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&barrier_id)
            .send()
            .await?;
        verify_that!(retrieved.body(), has_attribute(&name_iri, any_attribute().value("Eiffel Tower")))?;
        Ok(())
    })
    .await?;
    let linked = broker.with_context(Context::Single(contexts.reference(path)?))?;
    let kept = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = kept.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, lacks_attribute(&name_iri));
    expect_that!(contexts.downloads(path).await?, eq(first));
    Ok(())
}
