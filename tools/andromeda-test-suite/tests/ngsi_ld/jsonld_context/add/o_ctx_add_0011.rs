//! Subscriptions with the same context array share one implicit context.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::slice;
use url::Url;

/// Pins: Two subscriptions with the same @context array notified as json name the same jsonldContext, and the tenant
/// holds that array once, under a version 5 identifier the URL names
/// Clause: 5.13.1, 5.8.1, 5.13.3.4
/// Tenant: private (the case counts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: clause 5.13.1 leaves the format of a stored @context's local identifier to the broker; Andromeda names
/// every entry urn:ngsi-ld:JsonLdContext:<uuid>, a random (version 4) UUID for a Hosted or Cached entry and the
/// version 5 UUID of the canonical content for an `ImplicitlyCreated` one
#[gtest]
#[tokio::test]
async fn o_ctx_add_0011_subscriptions_with_the_same_context_array_share_one_implicit_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let probe_type = namespace.entity_type("IdentityProbe")?;
    let body_context = tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
            ContextEntry::Inline(format!(r#"{{"IdentityProbe_{namespace}": "https://test.andromeda/{namespace}/IdentityProbe"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    for id in [&subscription_id, &subscription_2_id] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([probe_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        body_context.subscriptions().create(&subscription).send().await?;
    }
    let first = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let first_url = first.jsonld_context();
    expect_that!(first_url.map(Url::as_str), some(contains_substring("/ngsi-ld/v1/jsonldContexts/")));
    let second = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(second.jsonld_context(), eq(first_url));
    let first_id = first_url
        .map(|url| HostedContext::listed(slice::from_ref(url), "jsonldContexts/"))
        .transpose()?
        .map(|stored| stored.id);
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    let implicit = HostedContext::listed(&contexts, "JsonLdContext")?;
    expect_that!(
        implicit.id.to_string(),
        matches_regex("^urn:ngsi-ld:JsonLdContext:[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    );
    expect_that!(Some(implicit.id), eq(&first_id));
    Ok(())
}
