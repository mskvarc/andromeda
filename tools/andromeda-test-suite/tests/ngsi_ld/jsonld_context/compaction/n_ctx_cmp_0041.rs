//! A subscription without a user context is notified under the core context.

use andromeda_test_suite::{
    broker,
    core_context::{core_context, core_entry},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, EntityTypeName, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: An application/ld+json notification of a subscription created under the core @context alone names the core
/// @context alone on the Notification and on the Entity
/// Clause: 5.8.1.4, 5.2.3, 5.5.5
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0041_a_subscription_without_a_user_context_is_notified_under_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let device_notified_id = namespace.urn("Device", "notified")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
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
    expect_that!(notification.context(), some(eq(&Context::Single(core_entry()?))));
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(item.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&device_notified_id)));
    Ok(())
}
