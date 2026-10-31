//! Notification over MQTT as the conformance tests drive it: the endpoint settings a test varies, and a Building update
//! observed through the MQTT listener.

use crate::building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    mqtt_observer::{MqttObserver, ReceivedMessage},
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::send_to,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        operation::{CreateEntity, CreateSubscription, UpdateAttributes},
    },
    model::{EntityFragment, Property},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use rumqttc::v5::mqttbytes::QoS;
use std::{fmt::Debug, time::Duration};
use tokio::time::sleep;

/// The port of the MQTT listener that an endpoint leaves out.
const MQTT_DEFAULT_PORT: u16 = 1883;

/// The Mosquitto configuration that the broker of an MQTT endpoint runs with.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttConfiguration {
    /// `mosquitto.conf`: anonymous clients allowed.
    Anonymous,
    /// `mosquitto_with_user.conf`: a password file, anonymous clients refused.
    WithUser,
}

/// The MQTT listener port that an endpoint names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttListener {
    /// 1883, which the endpoint leaves out.
    Default,
    /// The non-default listener port, which the endpoint names.
    NonDefault,
}

/// The protocol version that the `MQTT-Version` setting of an endpoint names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttVersion {
    /// MQTT 3.1.1, written `mqtt3.1.1`.
    V311,
    /// MQTT 5.0, written `mqtt5.0`.
    V50,
}

/// A `notifierInfo` setting of an MQTT endpoint (clause 7.2).
#[derive(Debug, Clone, Copy)]
pub(crate) enum MqttSetting {
    /// `MQTT-QoS`: the quality of service the notification is published with (Table 7.2-1).
    Qos(QoS),
    /// `MQTT-Version`: the protocol version the notification is published with.
    Version(MqttVersion),
}

impl MqttSetting {
    /// The key and the value of the `notifierInfo` pair the setting writes.
    const fn pair(self) -> (&'static str, &'static str) {
        match self {
            Self::Qos(QoS::AtMostOnce) => ("MQTT-QoS", "0"),
            Self::Qos(QoS::AtLeastOnce) => ("MQTT-QoS", "1"),
            Self::Qos(QoS::ExactlyOnce) => ("MQTT-QoS", "2"),
            Self::Version(MqttVersion::V311) => ("MQTT-Version", "mqtt3.1.1"),
            Self::Version(MqttVersion::V50) => ("MQTT-Version", "mqtt5.0"),
        }
    }
}

/// Publishes an attribute update of the Building `urn:ngsi-ld:Building:<ns>-1` through the broker and listener that
/// `configuration` and `listener` name. The subscription's endpoint carries `settings` as `notifierInfo` and
/// `receiver` as `receiverInfo`. Answers the last message the observer received, once one has arrived.
pub(crate) async fn mqtt_notification(
    userinfo: &str,
    configuration: MqttConfiguration,
    listener: MqttListener,
    settings: &[MqttSetting],
    receiver: &[(&str, &str)],
) -> Result<ReceivedMessage> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let mqtt = SuiteConfig::get()?.mqtt()?;
    let host = match configuration {
        MqttConfiguration::Anonymous => mqtt.host(),
        MqttConfiguration::WithUser => mqtt.auth_host(),
    };
    let (authority, listener_port) = match listener {
        MqttListener::Default => (host.to_string(), MQTT_DEFAULT_PORT),
        MqttListener::NonDefault => (format!("{host}:{}", mqtt.non_default_port()), mqtt.non_default_port()),
    };
    let topic = format!("{namespace}/ngsild-test-suite/topic");
    let observer = MqttObserver::subscribe(&host.to_string(), mqtt.observer_port(), listener_port, &topic, namespace.token()).await?;
    let endpoint = receiver
        .iter()
        .fold(
            settings
                .iter()
                .fold(Endpoint::builder(format!("mqtt://{userinfo}{authority}/{topic}")), |builder, setting| {
                    let (key, value) = setting.pair();
                    builder.notifier_info(key, value)
                }),
            |builder, &(key, value)| builder.receiver_info(key, value),
        )
        .build()?;

    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);

    let building = building_simple_attributes(&namespace, 1)?;
    let created = send_to(&as_string, &CreateEntity::builder().entity(&building).build(), "entities/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    sleep(Duration::from_secs(1)).await;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    // The selector takes a copy of the building's identifier; the building keeps its own for the update and the teardown.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let _subscription = Cleanup::of(&broker).subscription(&id);
    let subscribed = send_to(&as_list, &CreateSubscription { subscription: &subscription }, "subscriptions/").await;
    verify_that!(&subscribed, answered(StatusCode::CREATED))?;
    sleep(Duration::from_secs(1)).await;
    observer.clear().await;

    let fragment = EntityFragment::builder()
        .property(
            "airQualityLevel",
            Property::builder(5).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
        )
        .build()?;
    let update = UpdateAttributes::builder().id(&building.id).fragment(&fragment).build();
    send_to(&as_list, &update, &format!("entities/{}/attrs/", building.id)).await.any_answer();

    let messages = observer.wait_for(1).await?;
    let message = messages.into_iter().last().or_fail()?;

    broker.subscriptions().delete(&id).send().await.any_answer();
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(message)
}
