//! List an implicitly created context details false kind hosted is empty.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{ContextKindFilter, SummaryDetails},
    },
    model::{Context, ContextEntry, InlineContext},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: List @contexts with details=false and kind=Hosted, after a subscription has left an `ImplicitlyCreated`
/// @context, answers 200 with an empty list
/// Clause: 5.13.3.4, 6.29.3.2, 5.13.1
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.1 gives a subscription whose @context is an array as an example of an `ImplicitlyCreated`
/// @context; Andromeda hosts one wrapper for it when the subscription is created
#[gtest]
#[tokio::test]
async fn o_ctx_lst_0015_list_an_implicitly_created_context_details_false_kind_hosted_is_empty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"T1_{namespace}": "https://test.andromeda/{namespace}/T1"}}"#).parse::<InlineContext>()?),
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let t1_type = namespace.entity_type("T1")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([t1_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).build()?).build()?)
        .build()?;
    tenant_body_context.subscriptions().create(&subscription).send().await?;
    // Exactly one entry besides the core @context: a second one would make the filter answer a list.
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    HostedContext::listed(&contexts, "JsonLdContext")?;
    expect_that!(contexts, contains(displays_as(contains_substring("JsonLdContext"))).times(eq(1)));
    let contexts_2 = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .details(SummaryDetails::Explicit)
        .send()
        .await?;
    expect_that!(contexts_2, len(eq(0)));
    Ok(())
}
