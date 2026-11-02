//! An mqtt notification over the configured packet limit, to a server that announces no smaller maximum, is refused under that limit.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, MetricLabel, MetricsReading},
    config::{BrokerRole, SuiteConfig},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationStatus, NotificationTrigger, Subscription},
};
use url::Url;

/// The octets of each of the two values the entity is grown by: each write stays under the broker's 1800 `KiB`
/// `request_limits.max_body`, and the entity notified after the second carries both, over its 2 `MiB`
/// `max_mqtt_packet`.
const VALUE_OCTETS: usize = 1_200_000;

/// Pins: an MQTT 3.1.1 Subscription, whose server announces no maximum packet size, is notified of an Entity grown
/// past the broker's 2 `MiB` `max_mqtt_packet` by two writes each within its 1800 `KiB` `request_limits.max_body`:
/// the Subscription reports one failure and no success, and the broker counts one notification refused before sending
/// under `max_mqtt_packet`
/// Clause: 5.8.6, 5.2.14
/// Recorded: Andromeda extension, not v1.9.1: `notifications.delivery.max_mqtt_packet` and the
/// `andromeda_notification_oversized_total` counter on the management port; an MQTT 3.1.1 CONNACK carries no Maximum
/// Packet Size, so the configured limit is the only one that bounds the connection
#[gtest]
#[tokio::test]
async fn a_obs_nsz_9002_an_mqtt_notification_over_max_mqtt_packet_is_refused_under_that_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Oversized)?;
    let config = SuiteConfig::get()?;
    let capped_host = config.capped_mqtt_host()?.to_string();
    let before = MetricsReading::of(BrokerRole::Oversized).await?;
    let configured_before = before
        .sum_labelled(BrokerMetric::NotificationsOversized, MetricLabel::Limit, "max-mqtt-packet")
        .unwrap_or_default();
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription_id = namespace.subscription_id("configured")?;
    let endpoint = Endpoint::builder(format!("mqtt://{capped_host}/{namespace}/configured").parse::<Url>()?)
        .accept(NgsiLdMediaType::Json)
        .notifier_info("MQTT-Version", "mqtt3.1.1")
        .notifier_info("MQTT-QoS", "1");
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["annex"])
        .notification_triggers([NotificationTrigger::AttributeCreated])
        .notification(NotificationParams::builder(endpoint.build()?).build()?)
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
    let after = MetricsReading::of(BrokerRole::Oversized).await?;
    expect_that!(
        after.sum_labelled(BrokerMetric::NotificationsOversized, MetricLabel::Limit, "max-mqtt-packet"),
        some(eq(configured_before + 1.0))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
