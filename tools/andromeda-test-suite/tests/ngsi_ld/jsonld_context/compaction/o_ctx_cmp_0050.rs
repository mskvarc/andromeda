//! A csource subscription with a context array is given a hosted wrapper.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    minted_wrapper::{listed_wrapper_ids, minted_at, served_document},
    namespace::Namespace,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription created with two @contexts in the body gets as jsonldContext the
/// absolute URL of a hosted wrapper of those @contexts followed by the core, listed `ImplicitlyCreated`
/// Clause: 5.11.2.4, 5.8.1.4, 5.13.1
/// Tenant: private (the case asserts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: Table 5.2.12-1 types jsonldContext as one URI and clause 5.8.1.4 initialises it from the @context of the
/// create; Andromeda stores a @context that is one URL as written and hosts anything else as an `ImplicitlyCreated`
/// wrapper of the value an application/ld+json body would carry, storing its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0050_a_csource_subscription_with_a_context_array_is_given_a_hosted_wrapper() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let core = config.core_context();
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
        ]))?
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
            "jc/user/importing.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{core}",
        {{
          "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
          "speed": "https://test.andromeda/{namespace}/speed"
        }}
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant_body_context.registration_subscriptions().create(&subscription).send().await?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let wrapper_url = retrieved.jsonld_context().or_fail()?;
    let wrapper = minted_at(wrapper_url)?;
    let document = served_document(&tenant, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        document.context,
        eq(&Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
            core_entry()?
        ]))
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
