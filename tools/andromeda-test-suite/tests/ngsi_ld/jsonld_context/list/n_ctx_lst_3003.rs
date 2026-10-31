//! List contexts answers every cached context beyond a thousand.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    context_listing_walk::{ListingWalk, entry_count},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::header::CACHE_CONTROL;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity},
};

/// Pins: List @contexts of kind Cached, with and without details, walked by its next pointer from a page of 1000,
/// answers every Cached @context stored under its original URL, the first and the last of more than a thousand each
/// on exactly one page
/// Clause: 5.13.3.1, 5.13.3.3, 5.13.3.4, 6.29.3.2
/// Tenant: private (the subject is the completeness of the Cached listing, which only a tenant holding exactly the
/// downloads this file causes can show once more than 1000 exist)
#[gtest]
#[tokio::test]
async fn n_ctx_lst_3003_list_contexts_answers_every_cached_context_beyond_a_thousand() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve_all_under(
            "contexts/",
            &format!(r#"{{"@context": {{"cached": "https://test.andromeda/{namespace}/cached"}}}}"#),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "max-age=3600".parse()?),
        )
        .await?;
    download(&tenant, &contexts, &namespace, "first").await?;
    for index in 0..1000 {
        download(&tenant, &contexts, &namespace, &format!("many-{index}")).await?;
    }
    download(&tenant, &contexts, &namespace, "last").await?;
    let (first_url, last_url) = (contexts.url("contexts/first")?.to_string(), contexts.url("contexts/last")?.to_string());
    // Clause 5.13.3.1 lists a Cached @context under the URL it was downloaded from, clause 5.13.3.4 has the broker
    // provide an entry for each @context currently stored, and clause 5.5.9.1 pages the listing, so the walk over
    // its two pages holds the first and the last each on exactly one page. A download is recorded after the request
    // that made it, so the details listing is read until it holds every one.
    retried(async || {
        let walk = ListingWalk::read(&tenant, "limit=1000&kind=Cached&details=true").await?;
        verify_that!(entry_count(walk.first())?, eq(1000))?;
        verify_that!(entry_count(walk.second())?, ge(2))?;
        walk.require_on_one_page(&first_url)?;
        walk.require_on_one_page(&last_url)
    })
    .await?;
    let walk = ListingWalk::read(&tenant, "limit=1000&kind=Cached").await?;
    expect_that!(entry_count(walk.first())?, eq(1000));
    expect_that!(entry_count(walk.second())?, ge(2));
    walk.require_on_one_page(&first_url)?;
    walk.require_on_one_page(&last_url)?;
    Ok(())
}

/// Creates a Building whose @context is the document served at `contexts/<name>`, so the broker downloads it.
async fn download(tenant: &BrokerClient, contexts: &RemoteContexts, namespace: &Namespace, name: &str) -> Result<()> {
    let building = Entity::builder()
        .id(namespace.urn("Building", name)?)
        .entity_type(namespace.entity_type("Building")?)
        .build()?;
    tenant
        .with_context(Context::Multiple(vec![contexts.reference(&format!("contexts/{name}"))?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&building)
        .send()
        .await?
        .done()?;
    Ok(())
}
