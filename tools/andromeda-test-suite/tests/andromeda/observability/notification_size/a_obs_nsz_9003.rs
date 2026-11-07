//! An http notification over the configured body limit is refused under that limit.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, MetricLabel, MetricsReading},
    config::BrokerRole,
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

/// The octets of each of the two values the entity is grown by: each write stays under the broker's 1800 `KiB`
/// `request_limits.max_body`, and the entity notified after the second carries both, over its 1900 `KiB`
/// `max_http_body`.
const VALUE_OCTETS: usize = 1_000_000;

/// Pins: an HTTP Subscription is notified of an Entity grown past the broker's 1900 `KiB` `max_http_body` by two
/// writes each within its 1800 `KiB` `request_limits.max_body`: the endpoint receives nothing, the Subscription reports
/// one failure and no success, and the broker counts one notification refused before sending under `max_http_body`
/// Clause: 5.8.6, 5.2.14
/// Recorded: Andromeda extension, not v1.9.1: `notifications.delivery.max_http_body` and the
/// `andromeda_notification_oversized_total` counter on the management port
#[gtest]
#[tokio::test]
async fn a_obs_nsz_9003_an_http_notification_over_max_http_body_is_refused_under_that_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Oversized)?;
    let sink = NotificationSink::start().await?;
    let before = MetricsReading::of(BrokerRole::Oversized).await?;
    let configured_before = before
        .sum_labelled(BrokerMetric::NotificationsOversized, MetricLabel::Limit, "max-http-body")
        .unwrap_or_default();
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription_id = namespace.subscription_id("configured")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["annex"])
        .notification_triggers([NotificationTrigger::AttributeCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("configured")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("description", "x".repeat(VALUE_OCTETS))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let annex = EntityFragment::builder().property("annex", "y".repeat(VALUE_OCTETS)).build()?;
    broker.entities().append_attributes(&building_id, &annex).send().await?.done()?;
    retried(async || {
        let subscription = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(subscription.notification().output().times_failed, some(eq(1)))
    })
    .await?;
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let notification = subscription.notification().output();
    expect_that!(notification.status, some(eq(NotificationStatus::Failed)));
    expect_that!(notification.times_failed, some(eq(1)));
    expect_that!(notification.last_success, none());
    expect_that!(sink.received("configured").await?, len(eq(0)));
    let after = MetricsReading::of(BrokerRole::Oversized).await?;
    expect_that!(
        after.sum_labelled(BrokerMetric::NotificationsOversized, MetricLabel::Limit, "max-http-body"),
        some(eq(configured_before + 1.0))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
