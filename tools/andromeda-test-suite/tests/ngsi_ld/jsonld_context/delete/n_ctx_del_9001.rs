//! Reload a cached context replaces it with the new download.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_record,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextDeletion},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Delete @context with reload=true of a Cached @context answers 204 and replaces it with the document
/// downloaded then: an entity read under it shows the term the new document defines, a new entity written with that
/// term under it is stored with the IRI it maps to, and no further download is made
/// Clause: 5.13.5.4, 5.13.1
#[gtest]
#[tokio::test]
async fn n_ctx_del_9001_reload_a_cached_context_replaces_it_with_the_new_download() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    // The second download maps the stored name IRI to the term designation.
    contexts
        .serve_then(
            "ctx/reload.jsonld",
            (
                &format!(
                    r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}}}"#
                ),
                ServedAnswer::ok().with_media_type("application/ld+json"),
            ),
            (
                &format!(
                    r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "designation": "https://test.andromeda/{namespace}/name"}}}}"#
                ),
                ServedAnswer::ok().with_media_type("application/ld+json"),
            ),
        )
        .await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker
        .with_context(Context::Multiple(vec![contexts.reference("ctx/reload.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&building)
        .send()
        .await?
        .done()?;
    let url = contexts.url("ctx/reload.jsonld")?;
    // A download is recorded after the request that made it, so its entry is read until it appears.
    context_record::cached_under(&broker, &url).await?;
    let record = context_record::stored(&broker, &context_record::segment_of(&url)).await?;
    broker.contexts().delete(&record.local_id).deletion(ContextDeletion::Reload).send().await?;
    let reread = broker
        .with_context(Context::Single(contexts.reference("ctx/reload.jsonld")?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(reread.body(), has_attribute("designation", any_attribute().value("Eiffel Tower")));
    expect_that!(reread.body(), lacks_attribute("name"));
    let second_id = namespace.urn("Building", 2)?;
    let second = Entity::builder()
        .id(second_id.clone())
        .entity_type(building_type)
        .property("designation", "Arc de Triomphe")
        .build()?;
    broker
        .with_context(Context::Multiple(vec![contexts.reference("ctx/reload.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&second)
        .send()
        .await?
        .done()?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&second_id)
        .send()
        .await?;
    expect_that!(
        stored.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/name"), any_attribute().value("Arc de Triomphe"))
    );
    expect_that!(contexts.downloads("ctx/reload.jsonld").await?, eq(2));
    Ok(())
}
