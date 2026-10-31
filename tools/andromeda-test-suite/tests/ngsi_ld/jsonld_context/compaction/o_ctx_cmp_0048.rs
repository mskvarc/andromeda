//! A subscription with an inline context is given a hosted wrapper.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    context_terms::{defines_terms, listed_entries},
    core_context::core_entry,
    minted_wrapper::{listed_wrapper_ids, minted_at, served_document},
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription created with an inline @context definition gets as jsonldContext the absolute URL of a hosted
/// wrapper of that definition followed by the core, listed `ImplicitlyCreated`
/// Clause: 5.8.1.4, 5.13.1
/// Tenant: private (the case asserts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: Table 5.2.12-1 types jsonldContext as one URI and clause 5.8.1.4 initialises it from the @context of the
/// create; Andromeda stores a @context that is one URL as written and hosts anything else as an `ImplicitlyCreated`
/// wrapper of the value an application/ld+json body would carry, storing its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0048_a_subscription_with_an_inline_context_is_given_a_hosted_wrapper() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    let device_term = format!("Device_{namespace}");
    let tenant_body_context = tenant
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"{device_term}": "{device_iri}", "speed": "{speed_iri}"}}"#).parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([device_iri.clone()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    tenant_body_context.subscriptions().create(&subscription).send().await?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let wrapper_url = retrieved.jsonld_context().or_fail()?;
    let wrapper = minted_at(wrapper_url)?;
    let document = served_document(&tenant, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        listed_entries(&document.context),
        some(elements_are![
            defines_terms([(device_term.as_str(), device_iri.as_str()), ("speed", speed_iri.as_str())]),
            eq(&core_entry()?)
        ])
    );
    let listed = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listed)?, contains(eq(&wrapper.id)));
    Ok(())
}
