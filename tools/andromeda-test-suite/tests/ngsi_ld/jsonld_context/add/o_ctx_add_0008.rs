//! A subscription with a context array leaves an implicitly created context.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with an @context array of two inline definitions answers 201 and leaves an
/// `ImplicitlyCreated` @context in the tenant's listing
/// Clause: 5.13.1, 5.8.1, 5.13.3.4
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.1 gives a subscription whose @context is an array as an example of an `ImplicitlyCreated`
/// @context; Andromeda hosts one wrapper for it when the subscription is created
#[gtest]
#[tokio::test]
async fn o_ctx_add_0008_a_subscription_with_a_context_array_leaves_an_implicitly_created_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"T1_{namespace}": "https://test.andromeda/{namespace}/T1"}}"#).parse::<InlineContext>()?),
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let t1_type = namespace.entity_type("T1")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
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
    Ok(())
}
