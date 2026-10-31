//! Serve counts each use of an implicitly created context.

use andromeda_test_suite::{broker, config::SuiteConfig, context_details, hosted_context::HostedContext, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, Entity, InlineContext},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A create naming an `ImplicitlyCreated` @context raises its `numberOfHits` by the time the create has
/// answered
/// Clause: 5.13.3.5, 5.13.4.4, 5.13.1
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.1 gives a subscription whose @context is an array as an example of an `ImplicitlyCreated`
/// @context; Andromeda hosts one wrapper for it when the subscription is created
/// Recorded: `numberOfHits` is optional (clause 5.13.3.5); Andromeda reports it, and counts a use before the
/// request that made it returns
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0005_serve_counts_each_use_of_an_implicitly_created_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([namespace.entity_type("T1")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?).build()?)
        .build()?;
    tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"T1_{namespace}": "https://test.andromeda/{namespace}/T1"}}"#).parse::<InlineContext>()?),
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .subscriptions()
        .create(&subscription)
        .send()
        .await?;
    // Exactly one entry besides the core @context: a second one would make the filter answer a list.
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    let wrapper = HostedContext::listed(&contexts, "JsonLdContext")?;
    let before = context_details::served(&tenant, &wrapper.id).await?;
    let hits = before.number_of_hits.or_fail()?;
    let entity = Entity::builder()
        .id(namespace.urn("T1", 1)?)
        .entity_type(namespace.entity_type("T1")?)
        .property("speed", 1)
        .build()?;
    tenant
        .with_context(Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::new(
            before.url.as_str().to_owned(),
        )?)]))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&entity)
        .send()
        .await?
        .done()?;
    let after = context_details::served(&tenant, &wrapper.id).await?;
    expect_that!(after.number_of_hits, some(gt(hits)));
    Ok(())
}
