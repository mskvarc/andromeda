//! An expires of zero is already expired.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::header::EXPIRES;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, Entity, NgsiLdMediaType},
};
use url::Url;

/// Pins: A remote @context served with Expires: 0 is already expired, so it is downloaded again for the next use, and
/// it is listed under no kind
/// Clause: 6.3.16, 5.13.3.4, RFC 7234 s5.3
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9011_an_expires_of_zero_is_already_expired() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let path = "ctx/expires-zero.jsonld";
    let building_type = namespace.entity_type("Building")?;
    contexts
        .serve_answer(
            path,
            &format!(
                r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}}}"#
            ),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(EXPIRES, "0".parse()?),
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
    let linked = broker.with_context(Context::Single(contexts.reference(path)?))?;
    let kept = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(kept.body(), has_attribute("name", any_attribute().value("Eiffel Tower")));
    let first = contexts.downloads(path).await?;
    expect_that!(first, ge(1));
    let building_2_id = namespace.urn("Building", 2)?;
    let building_2 = Entity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .build()?;
    body_context.entities().create(&building_2).send().await?.done()?;
    expect_that!(contexts.downloads(path).await?, gt(first));
    let listed = broker
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Cached)
        .send()
        .await?;
    let url = contexts.url(path)?;
    verify_that!(listed.iter().map(Url::as_str).any(|listed| listed == url.as_str()), eq(false))?;
    Ok(())
}
