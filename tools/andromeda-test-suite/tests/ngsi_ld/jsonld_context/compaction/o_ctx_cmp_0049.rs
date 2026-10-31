//! A subscription with one context url stores it or a wrapper ending with the core.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    minted_wrapper::{listed_wrapper_ids, minted_at, served_document},
    namespace::Namespace,
    notification_sink::NotificationSink,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription created under one URL ending with the core @context stores that URL as jsonldContext, and one
/// created under a user @context without the core gets a hosted wrapper of it followed by the core
/// Clause: 5.8.1.4, 5.13.1, Annex C.8
/// Tenant: private (the case asserts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: Table 5.2.12-1 types jsonldContext as one URI and clause 5.8.1.4 initialises it from the @context of the
/// create; Andromeda stores a @context that is one URL as written and hosts anything else as an `ImplicitlyCreated`
/// wrapper of the value an application/ld+json body would carry, storing its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0049_a_subscription_with_one_context_url_stores_it_or_a_wrapper_ending_with_the_core() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let tenant_body_context = tenant
        .with_context(Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_2 = tenant
        .with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    tenant.registrations().delete(&registration_provision_id).send().await?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/user/wrapper.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([device_iri.clone()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    tenant_body_context.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([device_iri.clone()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    tenant_body_context_2.subscriptions().create(&subscription_2).send().await?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.jsonld_context(), some(eq(&contexts.url("jc/user/wrapper.jsonld")?)));
    let retrieved_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    let wrapper_url = retrieved_2.jsonld_context().or_fail()?;
    let wrapper = minted_at(wrapper_url)?;
    let document = served_document(&tenant, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        document.context,
        eq(&Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?]))
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
