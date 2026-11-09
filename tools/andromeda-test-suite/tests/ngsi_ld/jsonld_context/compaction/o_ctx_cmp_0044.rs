//! A core release as jsonld context is named as written in a notification.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::{core_context, core_entry},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, ContextEntry, Entity, EntityTypeName, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: An application/ld+json notification of a subscription whose `jsonldContext` is the v1.8 core, the
/// versionless URL or the v1.9 core names it as written on the Notification and on the Entity
/// Clause: 5.8.1.4, 5.2.3, 4.4
/// Recorded: clause 5.8.1.4 has the notification use the @context in `jsonldContext`; Andromeda names a
/// `jsonldContext` that is itself a core release, the versionless URL included, as written, as the subscriber's
/// explicit choice
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0044_a_core_release_as_jsonld_context_is_named_as_written_in_a_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let device_notified_id = namespace.urn("Device", "notified")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .jsonld_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")
        .build()?;
    core_context_body.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .jsonld_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld")
        .build()?;
    core_context_body.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id)
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .jsonld_context(config.core_context().clone())
        .build()?;
    core_context_body.subscriptions().create(&subscription_3).send().await?;
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
        some(eq(&Context::Single(ContextEntry::Reference(IriRefBuf::new(
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned()
        )?))))
    );
    let item = notification.data().first().or_fail()?;
    expect_that!(
        item.context.as_ref(),
        some(eq(&Context::Single(ContextEntry::Reference(IriRefBuf::new(
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned()
        )?))))
    );
    expect_that!(notification.data().as_slice(), nth(0, has_id(&device_notified_id)));
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.context(),
        some(eq(&Context::Single(ContextEntry::Reference(IriRefBuf::new(
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_owned()
        )?))))
    );
    let item_2 = notification_2.data().first().or_fail()?;
    expect_that!(
        item_2.context.as_ref(),
        some(eq(&Context::Single(ContextEntry::Reference(IriRefBuf::new(
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_owned()
        )?))))
    );
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&device_notified_id)));
    sink.wait_for_at_least(3, 1).await?;
    let notifications_3 = sink.received(3).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_3.context(), some(eq(&Context::Single(core_entry()?))));
    let item_3 = notification_3.data().first().or_fail()?;
    expect_that!(item_3.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(notification_3.data().as_slice(), nth(0, has_id(&device_notified_id)));
    Ok(())
}
