//! A fresh download is fetched once for two requests.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_record::cached_under,
    entity_matcher::has_attribute,
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

/// Pins: Two creates naming a remote @context served with Cache-Control max-age=3600 fetch it once, and the
/// download is recorded as Cached
/// Clause: 6.3.16, 5.13.1
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0023_a_fresh_download_is_fetched_once_for_two_requests() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve_answer(
            "jc/fresh.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "max-age=3600".parse()?),
        )
        .await?;
    let body_context = broker
        .with_context(Context::Single(contexts.reference("jc/fresh.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    for suffix in [8, 9] {
        let vehicle_id = namespace.urn("Vehicle", suffix)?;
        let vehicle = Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(namespace.entity_type("Vehicle")?)
            .property("speed", 1)
            .build()?;
        body_context.entities().create(&vehicle).send().await?.done()?;
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await?;
        expect_that!(
            retrieved.body(),
            has_attribute(&format!("https://test.andromeda/{namespace}/remote/speed"), any_attribute().value(1))
        );
    }
    expect_that!(contexts.downloads("jc/fresh.jsonld").await?, eq(1));
    cached_under(&broker, &contexts.url("jc/fresh.jsonld")?).await?;
    Ok(())
}
