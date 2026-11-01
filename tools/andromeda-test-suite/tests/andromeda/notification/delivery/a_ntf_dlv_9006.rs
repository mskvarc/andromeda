//! A notification over the http body limit fails once without a retry.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationStatus, NotificationTrigger, Subscription},
};

/// Pins: Under durable delivery, a notification larger than the HTTP body limit, of an entity grown past it by writes each within the request body limit, is never sent and fails on its first attempt without a retry, while the notification of the entity's creation to an unreachable endpoint is retried until its attempts run out
/// Clause: 5.8.6, 5.2.14
/// Recorded: Andromeda extension, not v1.9.1: `notifications.delivery.max_http_body` (5 KiB on broker-single-tenant, above its 4 KiB `request_limits.max_body`) under durable delivery with four attempts 100 ms apart
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_9006_a_notification_over_the_http_body_limit_fails_once_without_a_retry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_notification_limits = broker::client(BrokerRole::SingleTenant)?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_large_id = namespace.subscription_id("large")?;
    let subscription_unreachable_id = namespace.subscription_id("unreachable")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription_large = Subscription::builder()
        .id(subscription_large_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["annex"])
        .notification_triggers([NotificationTrigger::AttributeCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("large")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_notification_limits.subscriptions().create(&subscription_large).send().await?;
    let subscription_unreachable = Subscription::builder()
        .id(subscription_unreachable_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("{unreachable}/notify/{namespace}"))
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .attributes(["name"])
            .build()?,
        )
        .build()?;
    broker_notification_limits.subscriptions().create(&subscription_unreachable).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tower")
        .build()?;
    broker_notification_limits.entities().create(&building).send().await?.done()?;
    // Each write stays under the 4 KiB request body limit; the entity notified after the second carries both
    // 3000-character values and passes the 5 KiB notification body limit.
    let description = EntityFragment::builder().property("description", "x".repeat(3000)).build()?;
    broker_notification_limits
        .entities()
        .append_attributes(&building_id, &description)
        .send()
        .await?
        .done()?;
    let annex = EntityFragment::builder().property("annex", "y".repeat(3000)).build()?;
    broker_notification_limits
        .entities()
        .append_attributes(&building_id, &annex)
        .send()
        .await?
        .done()?;
    retried(async || {
        let subscription = broker_notification_limits
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_unreachable_id)
            .send()
            .await?;
        verify_that!(subscription.notification().output().times_failed, some(eq(4)))
    })
    .await?;
    let subscription_2 = broker_notification_limits
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_large_id)
        .send()
        .await?;
    let notification = subscription_2.notification().output();
    expect_that!(notification.status, some(eq(NotificationStatus::Failed)));
    expect_that!(notification.times_failed, some(eq(1)));
    expect_that!(notification.last_failure, some(anything()));
    expect_that!(notification.last_success, none());
    expect_that!(sink.received("large").await?, len(eq(0)));
    broker_notification_limits.subscriptions().delete(&subscription_large_id).send().await?;
    broker_notification_limits.subscriptions().delete(&subscription_unreachable_id).send().await?;
    Ok(())
}
