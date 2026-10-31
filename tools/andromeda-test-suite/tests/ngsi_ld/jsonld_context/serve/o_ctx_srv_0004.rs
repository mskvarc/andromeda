//! Serve with details answers the metadata of an implicitly created context.

use andromeda_test_suite::{broker, config::SuiteConfig, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext, StoredContextKind},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Serve @context with details=true of an `ImplicitlyCreated` entry answers 200 with its kind, a URL and a
/// `createdAt`
/// Clause: 5.13.4.4, 5.13.3.5, 5.13.1
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.1 gives a subscription whose @context is an array as an example of an `ImplicitlyCreated`
/// @context; Andromeda hosts one wrapper for it when the subscription is created
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0004_serve_with_details_answers_the_metadata_of_an_implicitly_created_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"T1_{namespace}": "https://test.andromeda/{namespace}/T1"}}"#).parse::<InlineContext>()?),
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([namespace.entity_type("T1")?.to_string()])
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
    let w = HostedContext::listed(&contexts, "JsonLdContext")?;
    expect_that!(contexts, contains(displays_as(contains_substring("JsonLdContext"))).times(eq(1)));
    let answer = context_details::serve_details(&tenant, &w.id).await?;
    expect_that!(context_details::metadata(&answer)?.kind, eq(&StoredContextKind::ImplicitlyCreated));
    Ok(())
}
