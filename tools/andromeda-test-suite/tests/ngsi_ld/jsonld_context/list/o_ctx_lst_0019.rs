//! A tenant with entries holds no core context entry.

use andromeda_test_suite::{broker, config::SuiteConfig, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{ContextKindFilter, SummaryDetails},
    },
    model::{Context, ContextEntry, ContextMetadata, InlineContext},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: List @contexts, in a tenant holding a Hosted entry and an `ImplicitlyCreated` wrapper, answers 200 and no
/// listing variant names the core @context
/// Clause: 5.13.3.4, 5.13.1, 6.29.3.2
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.3.4 lists "@contexts currently stored"; the core @context is not a stored entry, so no
/// listing variant names its URL
#[gtest]
#[tokio::test]
async fn o_ctx_lst_0019_a_tenant_with_entries_holds_no_core_context_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let core = config.core_context();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    HostedContext::add(
        &tenant,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Hosted_{namespace}": "https://test.andromeda/{namespace}/Hosted"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
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
    let plain = tenant.with_accept(Accept::any()).contexts().list().send().await?;
    expect_that!(plain, not(contains(eq(core))));
    let explicit = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .details(SummaryDetails::Explicit)
        .send()
        .await?;
    expect_that!(explicit, not(contains(eq(core))));
    let implicit = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(implicit, not(contains(eq(core))));
    let hosted = context_details::listed(&tenant, Some(ContextKindFilter::Hosted)).await?;
    expect_that!(hosted, not(contains(field!(ContextMetadata.url, eq(core)))));
    Ok(())
}
