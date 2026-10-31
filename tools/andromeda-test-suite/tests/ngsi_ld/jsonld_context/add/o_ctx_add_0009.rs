//! A hosted context naming a remote url records only the download as cached.

use andromeda_test_suite::{
    broker,
    context_details,
    eventually::retried,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, operation::ContextKindFilter},
    model::{Context, ContextEntry, ContextMetadata, Entity, StoredContextKind},
};

/// Pins: Add @context with an array naming a remote URL stores a Hosted entry; a create naming the Hosted entry
/// downloads the remote URL and records it as Cached, and never records the Hosted entry's own URL as Cached
/// Clause: 5.13.1, 5.13.2.4, 5.13.3.4
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_add_0009_a_hosted_context_naming_a_remote_url_records_only_the_download_as_cached() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "jc/c.jsonld",
            &format!(r#"{{"@context": {{"X1": "https://test.andromeda/{namespace}/X1"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let remote = contexts.url("jc/c.jsonld")?;
    let hosted = HostedContext::add(&tenant, &Context::Multiple(vec![contexts.reference("jc/c.jsonld")?])).await?;
    let record = context_details::metadata(&context_details::serve_details(&tenant, &hosted.id).await?)?;
    expect_that!(record.kind, eq(&StoredContextKind::Hosted));
    let hosted_url = record.url;
    let testing = Entity::builder()
        .id(namespace.urn("Testing", 1)?)
        .entity_type(namespace.entity_type("Testing")?)
        .property("X1", "test")
        .build()?;
    tenant
        .with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(hosted_url.as_str().to_owned())?)))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&testing)
        .send()
        .await?
        .done()?;
    // A download is recorded after the request that made it, so the listing is read until it shows it.
    retried(async || {
        let cached = context_details::listed(&tenant, Some(ContextKindFilter::Cached)).await?;
        verify_that!(
            cached,
            contains(all!(
                field!(ContextMetadata.url, eq(&remote)),
                field!(ContextMetadata.kind, eq(&StoredContextKind::Cached))
            ))
        )?;
        verify_that!(cached, not(contains(field!(ContextMetadata.url, eq(&hosted_url)))))
    })
    .await?;
    Ok(())
}
