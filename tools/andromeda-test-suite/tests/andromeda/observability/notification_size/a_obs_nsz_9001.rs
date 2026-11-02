//! An mqtt notification over the maximum packet size the mqtt server announced is refused under that limit.

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
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationStatus, NotificationTrigger, Subscription},
};
use url::Url;

/// The octets of the description value the entity carries: 1.5 `MiB`, over the 1 `MiB` maximum packet the capped
/// Mosquitto broker announces and under the broker's 1800 `KiB` `request_limits.max_body`, so the notification also
/// stays under its 2 `MiB` `max_mqtt_packet`.
const DESCRIPTION_OCTETS: usize = 1_572_864;

/// Pins: an MQTT 5.0 Subscription to a server whose CONNACK announces a 1 `MiB` maximum packet size, below the
/// broker's 2 `MiB` `max_mqtt_packet`, is notified of an Entity of 1.5 `MiB`: the Subscription reports one failure and
/// no success, and the broker counts one notification refused before sending under the limit the server announced
/// Clause: 5.8.6, 5.2.14
/// Recorded: Andromeda extension, not v1.9.1: `notifications.delivery.max_mqtt_packet` and the
/// `andromeda_notification_oversized_total` counter on the management port; MQTT 5.0 section 3.2.2.3.6 forbids a client
/// to send a packet larger than the Maximum Packet Size the server announced
#[gtest]
#[tokio::test]
async fn a_obs_nsz_9001_an_mqtt_notification_over_the_announced_maximum_is_refused_under_that_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Oversized)?;
    let config = SuiteConfig::get()?;
    let capped_host = config.capped_mqtt_host()?.to_string();
    let before = MetricsReading::of(BrokerRole::Oversized).await?;
    let announced_before = before
        .sum_labelled(BrokerMetric::NotificationsOversized, MetricLabel::Limit, "mqtt-server-announced")
        .unwrap_or_default();
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id("capped")?;
    let endpoint = Endpoint::builder(format!("mqtt://{capped_host}/{namespace}/capped").parse::<Url>()?)
        .accept(NgsiLdMediaType::Json)
        .notifier_info("MQTT-Version", "mqtt5.0")
        .notifier_info("MQTT-QoS", "1");
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(endpoint.build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("description", "x".repeat(DESCRIPTION_OCTETS))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
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
        after.sum_labelled(BrokerMetric::NotificationsOversized, MetricLabel::Limit, "mqtt-server-announced"),
        some(eq(announced_before + 1.0))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
