//! A jsonld context without the core context is followed by it in a notification.

use andromeda_test_suite::{
    broker,
    core_context::{core_context, core_entry},
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

/// Pins: An application/ld+json notification of a subscription whose `jsonldContext` is a user @context without the
/// core names it followed by the core @context on the Notification and on the Entity
/// Clause: 5.8.1.4, 5.2.3
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0043_a_jsonld_context_without_the_core_context_is_followed_by_it_in_a_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
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
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([format!("https://test.andromeda/{namespace}/Device")])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .jsonld_context(contexts.url("jc/user/terms.jsonld")?)
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
    expect_that!(notification.context().is_some_and(|context| matches!(context, Context::Multiple(_))), eq(true));
    expect_that!(notification.context().map(Context::entries).and_then(|entries| entries.get(2)), none());
    expect_that!(
        notification.context().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        notification.context().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&core_entry()?))
    );
    let item = notification.data().first().or_fail()?;
    expect_that!(item.context.as_ref().is_some_and(|context| matches!(context, Context::Multiple(_))), eq(true));
    expect_that!(item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(2)), none());
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&core_entry()?))
    );
    expect_that!(notification.data().as_slice(), nth(0, has_id(&device_notified_id)));
    Ok(())
}
