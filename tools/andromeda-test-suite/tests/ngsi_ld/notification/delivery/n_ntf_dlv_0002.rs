//! An mqtt notification is published at the qos the notifier info names.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, mqtt_observer::MqttObserver, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use rumqttc::v5::mqttbytes::QoS;
use url::Url;

/// Pins: A notification of an mqtt subscription is published at `QoS` 0 when `notifierInfo` names no `MQTT-QoS`,
/// and at `QoS` 1 or 2 when it names that value, each message carrying the Notification of its own subscription
/// Clause: 7.2, 5.8.6, 5.2.15
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_0002_an_mqtt_notification_is_published_at_the_qos_the_notifier_info_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let mqtt = config.mqtt()?;
    let host = mqtt.host().to_string();
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let cases = [(1, None, QoS::AtMostOnce), (2, Some("1"), QoS::AtLeastOnce), (3, Some("2"), QoS::ExactlyOnce)];
    let mut observers = Vec::new();
    for (n, info, _) in &cases {
        observers.push(
            MqttObserver::subscribe(
                &host,
                mqtt.observer_port(),
                1883,
                &format!("{namespace}/qos/{n}"),
                &format!("{namespace}-qos-{n}"),
            )
            .await?,
        );
        let endpoint = Endpoint::builder(format!("mqtt://{namespace}-{n}@{host}/{namespace}/qos/{n}").parse::<Url>()?).accept(NgsiLdMediaType::Json);
        let endpoint = match info {
            Some(qos) => endpoint.notifier_info("MQTT-QoS", *qos),
            None => endpoint,
        };
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(n)?)
            .entities([building_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(endpoint.build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    for ((n, _, qos), observer) in cases.iter().zip(&observers) {
        observer.wait_for(1).await?;
        let messages = observer.received().await;
        expect_that!(messages, len(eq(1)));
        let message = messages.first().or_fail()?;
        let notification = message.notification(Lossless::Normalized)?;
        expect_that!(notification.body.subscription_id(), eq(&namespace.subscription_id(n)?));
        expect_that!(notification.body.data().as_slice(), nth(0, has_id(&building_id)));
        expect_that!(message.qos, eq(*qos));
    }
    Ok(())
}
