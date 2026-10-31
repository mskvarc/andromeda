//! A jsonld context ending with the core context is named alone in a notification.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_context,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, EntityTypeName, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: An application/ld+json notification of a subscription whose `jsonldContext` is one URL ending with the
/// core @context names that URL alone on the Notification and on the Entity
/// Clause: 5.8.1.4, 5.2.3, Annex C.8
/// Recorded: clause 5.2.3 has an application/ld+json Notification name the user @context and the core @context;
/// Andromeda names `jsonldContext` followed by the v1.9 core unless `jsonldContext` leaves the core in force last
/// (Annex C.8)
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0042_a_jsonld_context_ending_with_the_core_context_is_named_alone_in_a_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let device_notified_id = namespace.urn("Device", "notified")?;
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
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .jsonld_context(contexts.url("jc/user/wrapper.jsonld")?)
        .build()?;
    core_context_body.subscriptions().create(&subscription).send().await?;
    let device_notified = Entity::builder()
        .id(device_notified_id.clone())
        .entity_type(format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?)
        .property(format!("https://test.andromeda/{namespace}/speed").as_str(), 3)
        .build()?;
    broker.entities().create(&device_notified).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.context(),
        some(eq(&Context::Single(contexts.reference("jc/user/wrapper.jsonld")?)))
    );
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(item.context.as_ref(), some(eq(&Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&device_notified_id)));
    Ok(())
}
