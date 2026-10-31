//! A notification of a 20 mib entity is published at qos 1 under either mqtt version.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    eventually::retried,
    identity_matcher::has_id,
    mqtt_observer::MqttObserver,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationStatus, NotificationTrigger, Subscription},
};
use rumqttc::v5::mqttbytes::QoS;
use url::Url;

/// The octets of the description value the entity carries: 20 `MiB`.
const DESCRIPTION_OCTETS: usize = 20_971_520;

/// The least octets of the published payload: the entity alone is 20 `MiB`, so the payload length has eight digits and
/// starts with 2.
const PAYLOAD_FLOOR: usize = 20_000_000;

/// The most octets of that payload.
const PAYLOAD_CEILING: usize = 29_999_999;

/// Pins: The creation of an Entity of 20 `MiB`, a body the broker accepts, is published to mqtt endpoints under MQTT 5.0
/// and MQTT 3.1.1 at the `QoS` 1 their `notifierInfo` names, in one message each carrying the Entity
/// Clause: 7.2, 5.8.6, 5.2.15
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_9005_a_notification_of_a_20_mib_entity_is_published_at_qos_1_under_either_mqtt_version() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let mqtt = config.mqtt()?;
    let large_host = mqtt.large_host()?.to_string();
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let versions = [("v5", "mqtt5.0"), ("v311", "mqtt3.1.1")];
    let mut observers = Vec::new();
    for (name, version) in &versions {
        observers.push(
            MqttObserver::subscribe_large(
                &large_host,
                mqtt.observer_port(),
                1883,
                &format!("{namespace}/{name}"),
                &format!("{namespace}-{name}"),
            )
            .await?,
        );
        let endpoint = Endpoint::builder(format!("mqtt://{large_host}/{namespace}/{name}").parse::<Url>()?)
            .accept(NgsiLdMediaType::Json)
            .notifier_info("MQTT-Version", *version)
            .notifier_info("MQTT-QoS", "1");
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(name)?)
            .entities([building_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(endpoint.build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("description", "x".repeat(DESCRIPTION_OCTETS))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    for ((name, _), observer) in versions.iter().zip(&observers) {
        observer.wait_for(1).await?;
        let messages = observer.received().await;
        expect_that!(messages, len(eq(1)));
        let message = messages.first().or_fail()?;
        expect_that!(message.qos, eq(QoS::AtLeastOnce));
        expect_that!(message.payload.len(), ge(PAYLOAD_FLOOR));
        expect_that!(message.payload.len(), le(PAYLOAD_CEILING));
        let notification = message.notification(Lossless::Normalized)?;
        expect_that!(notification.body.data().as_slice(), nth(0, has_id(&building_id)));
        let subscription_id = namespace.subscription_id(name)?;
        retried(async || {
            let subscription = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .subscriptions()
                .retrieve(&subscription_id)
                .send()
                .await?;
            verify_that!(subscription.notification().output().status, some(eq(NotificationStatus::Ok)))?;
            verify_that!(subscription.notification().output().times_sent, some(eq(1)))
        })
        .await?;
    }
    Ok(())
}
