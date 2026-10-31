//! Qos 1 publishes beside qos 0 publishes on one connection are acknowledged.

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
use url::Url;

/// Pins: Of one Entity creation, twelve `QoS` 1 and twelve `QoS` 0 mqtt subscriptions publishing through one broker
/// connection (same broker, port, version and user) each deliver one message, and every `QoS` 1 subscription reads
/// back with notification status ok and no `lastFailure`
/// Clause: 7.2, 5.8.6, 5.2.14.2
/// Tenant: private (a delivery that never sees its acknowledgement holds back the notifications behind it; the
/// `QoS` 1 endpoints set a timeout of 1000 ms to keep that short)
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_0004_qos_1_publishes_beside_qos_0_publishes_on_one_connection_are_acknowledged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let config = SuiteConfig::get()?;
    let mqtt = config.mqtt()?;
    let host = mqtt.host().to_string();
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let mut observers = Vec::new();
    for n in 1..=24 {
        observers.push(
            MqttObserver::subscribe(
                &host,
                mqtt.observer_port(),
                1883,
                &format!("{namespace}/mixed/{n}"),
                &format!("{namespace}-mixed-{n}"),
            )
            .await?,
        );
        let acknowledged = n % 2 == 1;
        let endpoint = Endpoint::builder(format!("mqtt://{namespace}@{host}/{namespace}/mixed/{n}").parse::<Url>()?)
            .accept(NgsiLdMediaType::Json)
            .maybe_timeout(acknowledged.then_some(1000));
        let endpoint = if acknowledged { endpoint.notifier_info("MQTT-QoS", "1") } else { endpoint };
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
    for (n, observer) in (1..=24).zip(&observers) {
        observer.wait_for(1).await?;
        let messages = observer.received().await;
        expect_that!(messages, len(eq(1)));
        let message = messages.first().or_fail()?;
        let notification = message.notification(Lossless::Normalized)?;
        expect_that!(notification.body.subscription_id(), eq(&namespace.subscription_id(n)?));
        expect_that!(notification.body.data().as_slice(), nth(0, has_id(&building_id)));
    }
    for n in (1..=23).step_by(2) {
        let subscription_id = namespace.subscription_id(n)?;
        retried(async || {
            let subscription = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .subscriptions()
                .retrieve(&subscription_id)
                .send()
                .await?;
            verify_that!(subscription.notification().output().status, some(eq(NotificationStatus::Ok)))?;
            verify_that!(subscription.notification().output().last_success, some(anything()))?;
            verify_that!(subscription.notification().output().last_failure, none())?;
            Ok(())
        })
        .await?;
    }
    Ok(())
}
