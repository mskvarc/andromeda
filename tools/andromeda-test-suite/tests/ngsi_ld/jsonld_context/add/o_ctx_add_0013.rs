//! Subscriptions with different context arrays get distinct implicit contexts.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext, Input, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::slice;
use url::Url;

/// Pins: Two subscriptions naming the same two definitions in opposite array order name different jsonldContexts, and
/// the tenant holds two wrappers, both under version 5 identifiers
/// Clause: 5.13.1, 5.8.1, JSON-LD 1.1 s4.1
/// Tenant: private (the case counts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: clause 5.13.1 leaves the format of a stored @context's local identifier to the broker; Andromeda names
/// every entry urn:ngsi-ld:JsonLdContext:<uuid>, a random (version 4) UUID for a Hosted or Cached entry and the
/// version 5 UUID of the canonical content for an `ImplicitlyCreated` one
#[gtest]
#[tokio::test]
async fn o_ctx_add_0013_subscriptions_with_different_context_arrays_get_distinct_implicit_contexts() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let probe_type = namespace.entity_type("IdentityProbe")?;
    let speed = || -> Result<ContextEntry> {
        Ok(ContextEntry::Inline(
            format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?,
        ))
    };
    let probe = || -> Result<ContextEntry> {
        Ok(ContextEntry::Inline(
            format!(r#"{{"IdentityProbe_{namespace}": "https://test.andromeda/{namespace}/IdentityProbe"}}"#).parse::<InlineContext>()?,
        ))
    };
    let forward_context = tenant
        .with_context(Context::Multiple(vec![speed()?, probe()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let reverse_context = tenant
        .with_context(Context::Multiple(vec![probe()?, speed()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let notification = || -> Result<NotificationParams<Input>> {
        Ok(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
    };
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .notification(notification()?)
        .build()?;
    forward_context.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([probe_type.as_str()])
        .notification(notification()?)
        .build()?;
    reverse_context.subscriptions().create(&subscription_2).send().await?;
    let forward = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        forward.jsonld_context().map(Url::as_str),
        some(contains_substring("/ngsi-ld/v1/jsonldContexts/"))
    );
    let reverse = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(reverse.jsonld_context(), not(eq(forward.jsonld_context())));
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    let wrappers = contexts.iter().filter(|url| url.as_str().contains("JsonLdContext")).collect::<Vec<_>>();
    expect_that!(wrappers, len(eq(2)));
    for url in wrappers {
        let wrapper = HostedContext::listed(slice::from_ref(url), "JsonLdContext")?;
        expect_that!(
            wrapper.id.to_string(),
            matches_regex("^urn:ngsi-ld:JsonLdContext:[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
        );
    }
    Ok(())
}
