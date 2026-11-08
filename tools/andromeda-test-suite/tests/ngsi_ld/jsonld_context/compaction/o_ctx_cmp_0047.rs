//! A subscription with a context array is notified under its hosted wrapper.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    header_condition::HeaderCondition,
    minted_wrapper::{listed_wrapper_ids, minted_at, served_document},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::header::LINK;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, Entity, EntityTypeName, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Two subscriptions created with the same two @contexts in the body get the same jsonldContext, the absolute
/// URL of a hosted wrapper of those @contexts followed by the core, listed `ImplicitlyCreated`; an ld+json
/// notification names that URL alone and a json one links it once
/// Clause: 5.8.1.4, 5.13.1, 6.3.8, Annex C.8
/// Tenant: private (the case asserts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: Table 5.2.12-1 types jsonldContext as one URI and clause 5.8.1.4 initialises it from the @context of the
/// create; Andromeda stores a @context that is one URL as written and hosts anything else as an `ImplicitlyCreated`
/// wrapper of the value an application/ld+json body would carry, storing its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0047_a_subscription_with_a_context_array_is_notified_under_its_hosted_wrapper() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let core = config.core_context();
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    let notified_id = namespace.urn("Device", "notified")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "{device_iri}",
        "speed": "{speed_iri}"
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
          "Device_{namespace}": "{device_iri}",
          "speed": "{speed_iri}"
        }}
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
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant_body_context.subscriptions().create(&subscription_2).send().await?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let wrapper_url = retrieved.jsonld_context().or_fail()?;
    let wrapper = minted_at(wrapper_url)?;
    let retrieved_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(retrieved_2.jsonld_context(), some(eq(&wrapper.url)));
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
    let notified = Entity::builder()
        .id(notified_id.clone())
        .entity_type(device_iri.parse::<EntityTypeName>()?)
        .property(speed_iri.as_str(), 3)
        .build()?;
    tenant.entities().create(&notified).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let received = sink.received(1).await?;
    expect_that!(received, len(eq(1)));
    let notification = received.first().or_fail()?.entities(Lossless::Normalized)?;
    let named = Context::Single(ContextEntry::Reference(IriRefBuf::new(wrapper.url.as_str().to_owned())?));
    expect_that!(notification.context(), some(eq(&named)));
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity.id, eq(&notified_id));
    expect_that!(entity.context.as_ref(), some(eq(&named)));
    sink.wait_for_at_least(2, 1).await?;
    let link_once = HeaderCondition::exactly(LINK, &format!("<{}>;[^,]*json-ld#context[^,]*", regex::escape(wrapper.url.as_str())))?;
    let received_2 = sink.received_matching(2, &[], &[link_once]).await?;
    expect_that!(received_2, len(eq(1)));
    let notification_2 = received_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.context(), none());
    Ok(())
}
