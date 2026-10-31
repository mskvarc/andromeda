//! An own shaped reference to an unheld identifier is fetched and never recorded.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_record::segment_of_location,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
};
use url::Url;

/// Pins: A create under a Link naming a path shaped like a stored @context of an identifier the tenant does not
/// hold fetches the document from that URL and expands through it, and the URL is never recorded as Cached
/// Clause: 5.13.1, 6.3.5
/// Tenant: private (the case asserts what the tenant records as Cached, and clause 4.14 keeps it the tenant's own)
/// Recorded: clause 5.13.1 has Hosted and `ImplicitlyCreated` @contexts served from the broker's storage; Andromeda
/// resolves a reference whose path is `/ngsi-ld/v1/jsonldContexts/<identifier` it holds>, with no query, from the
/// requesting tenant's storage whatever its origin, never fetches it and never records it as Cached
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0018_an_own_shaped_reference_to_an_unheld_identifier_is_fetched_and_never_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"ownSpeed": "https://test.andromeda/{namespace}/own/speed"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    let segment = segment_of_location(&ctx.location)?;
    tenant.contexts().delete(&ctx.id).send().await?;
    let at = format!("ngsi-ld/v1/jsonldContexts/{segment}");
    contexts
        .serve(
            &at,
            &format!(r#"{{"@context": {{"ownSpeed": "https://test.andromeda/{namespace}/own/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let linked = tenant.with_context(Context::Single(contexts.reference(&at)?))?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("ownSpeed", 7)
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/own/speed"), any_attribute().value(7))
    );
    expect_that!(contexts.downloads(&at).await?, ge(1));
    let listed = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Cached)
        .send()
        .await?;
    verify_that!(listed.iter().map(Url::as_str).any(|url| url.contains(segment)), eq(false))
}
