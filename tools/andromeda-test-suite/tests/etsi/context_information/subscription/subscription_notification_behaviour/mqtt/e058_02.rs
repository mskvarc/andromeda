//! ETSI `058_02`: a broker publishes an MQTT notification at the quality of service the subscription's `notifierInfo`
//! names (clause 5.8.6).

use crate::suite_keywords::{MqttConfiguration, MqttListener, MqttSetting, mqtt_notification};
use googletest::prelude::*;
use rumqttc::v5::mqttbytes::QoS;

/// Pins: Check that the broker supports different quality of service (qos)
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0, the default of clause 7.2, Table
/// 7.2-1, when notifierInfo names no MQTT-QoS; Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS 1
/// and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes at
/// QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for a
/// JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where Robot
/// only counts it
#[gtest]
#[tokio::test]
async fn e058_02_01_with_default_qos() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker supports different quality of service (qos)
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 1, the MQTT-QoS the subscription's
/// notifierInfo names (clause 7.2, Table 7.2-1); Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS
/// 1 and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes
/// at QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for
/// a JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where
/// Robot only counts it
#[gtest]
#[tokio::test]
async fn e058_02_02_with_qos_1() -> Result<()> {
    let message = mqtt_notification(
        "",
        MqttConfiguration::Anonymous,
        MqttListener::Default,
        &[MqttSetting::Qos(QoS::AtLeastOnce)],
        &[],
    )
    .await?;
    expect_that!(message.qos, eq(QoS::AtLeastOnce));
    Ok(())
}

/// Pins: Check that the broker supports different quality of service (qos)
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 2, the MQTT-QoS the subscription's
/// notifierInfo names (clause 7.2, Table 7.2-1); Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS
/// 1 and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes
/// at QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for
/// a JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where
/// Robot only counts it
#[gtest]
#[tokio::test]
async fn e058_02_03_with_qos_2() -> Result<()> {
    let message = mqtt_notification(
        "",
        MqttConfiguration::Anonymous,
        MqttListener::Default,
        &[MqttSetting::Qos(QoS::ExactlyOnce)],
        &[],
    )
    .await?;
    expect_that!(message.qos, eq(QoS::ExactlyOnce));
    Ok(())
}
