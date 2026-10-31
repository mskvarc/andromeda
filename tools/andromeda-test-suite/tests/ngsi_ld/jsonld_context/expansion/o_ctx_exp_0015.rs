//! An absolute reference on any origin is resolved from the requesting tenants storage.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
};

/// Pins: A create under a Link naming a stored Hosted @context by its path on an origin where nothing listens is
/// resolved from the tenant's storage without a fetch, and nothing is recorded as Cached
/// Clause: 5.13.1, 6.3.5
/// Tenant: private (the case asserts what the tenant records as Cached, and clause 4.14 keeps it the tenant's own)
/// Recorded: clause 5.13.1 has Hosted and `ImplicitlyCreated` @contexts served from the broker's storage; Andromeda
/// resolves a reference whose path is `/ngsi-ld/v1/jsonldContexts/<identifier it holds>`, with no query, from the
/// requesting tenant's storage whatever its origin, never fetches it and never records it as Cached
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0015_an_absolute_reference_on_any_origin_is_resolved_from_the_requesting_tenants_storage() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let unreachable = SuiteConfig::get()?.unreachable_origin().origin().ascii_serialization();
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"ownSpeed": "https://test.andromeda/{namespace}/own/speed"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}{}",
        ctx.url.path()
    ))?)))?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("ownSpeed", 7)
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/own/speed"), any_attribute().value(7))
    );
    let cached = broker
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Cached)
        .send()
        .await?;
    expect_that!(cached.len(), eq(0));
    Ok(())
}
