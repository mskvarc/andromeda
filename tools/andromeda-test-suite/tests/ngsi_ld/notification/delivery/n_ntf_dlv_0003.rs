//! An mqtt notification is delivered under every mqtt version.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, mqtt_observer::MqttObserver, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use url::Url;

/// Pins: A notification of an mqtt subscription is delivered when `notifierInfo` names no MQTT-Version,
/// MQTT-Version mqtt3.1.1 or MQTT-Version mqtt5.0, each message carrying the Notification of its own subscription
/// Clause: 7.2, 5.8.6, 5.2.15
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_0003_an_mqtt_notification_is_delivered_under_every_mqtt_version() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let mqtt = config.mqtt()?;
    let observer_version_1 = MqttObserver::subscribe(
        &mqtt.host().to_string(),
        mqtt.observer_port(),
        1883,
        &format!("{namespace}/version/1"),
        &format!("{namespace}-version-1"),
    )
    .await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let observer_version_2 = MqttObserver::subscribe(
        &mqtt.host().to_string(),
        mqtt.observer_port(),
        1883,
        &format!("{namespace}/version/2"),
        &format!("{namespace}-version-2"),
    )
    .await?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let observer_version_3 = MqttObserver::subscribe(
        &mqtt.host().to_string(),
        mqtt.observer_port(),
        1883,
        &format!("{namespace}/version/3"),
        &format!("{namespace}-version-3"),
    )
    .await?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("mqtt://{}/{namespace}/version/1", mqtt.host()).parse::<Url>()?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("mqtt://{}/{namespace}/version/2", mqtt.host()).parse::<Url>()?)
                    .accept(NgsiLdMediaType::Json)
                    .notifier_info("MQTT-Version", "mqtt3.1.1")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("mqtt://{}/{namespace}/version/3", mqtt.host()).parse::<Url>()?)
                    .accept(NgsiLdMediaType::Json)
                    .notifier_info("MQTT-Version", "mqtt5.0")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_3).send().await?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    observer_version_1.wait_for(1).await?;
    let messages = observer_version_1.received().await;
    expect_that!(messages, len(eq(1)));
    let message = messages.first().or_fail()?;
    let notification = message.notification(Lossless::Normalized)?;
    let body = &notification.body;
    expect_that!(body.subscription_id(), eq(&subscription_id));
    expect_that!(body.data().as_slice(), nth(0, has_id(&building_id)));
    observer_version_2.wait_for(1).await?;
    let messages_2 = observer_version_2.received().await;
    expect_that!(messages_2, len(eq(1)));
    let message_2 = messages_2.first().or_fail()?;
    let notification_2 = message_2.notification(Lossless::Normalized)?;
    let body_2 = &notification_2.body;
    expect_that!(body_2.subscription_id(), eq(&subscription_2_id));
    expect_that!(body_2.data().as_slice(), nth(0, has_id(&building_id)));
    observer_version_3.wait_for(1).await?;
    let messages_3 = observer_version_3.received().await;
    expect_that!(messages_3, len(eq(1)));
    let message_3 = messages_3.first().or_fail()?;
    let notification_3 = message_3.notification(Lossless::Normalized)?;
    let body_3 = &notification_3.body;
    expect_that!(body_3.subscription_id(), eq(&subscription_3_id));
    expect_that!(body_3.data().as_slice(), nth(0, has_id(&building_id)));
    Ok(())
}
