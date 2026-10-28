//! ETSI `058_03`: a broker publishes an MQTT notification with the protocol version that the subscription's
//! `notifierInfo` names (clause 5.8.6).

use crate::suite_keywords::{MqttConfiguration, MqttListener, MqttSetting, MqttVersion, mqtt_notification};
use googletest::prelude::*;
use rumqttc::v5::mqttbytes::QoS;

/// Pins: Check that the broker support different mqtt version
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0, the default of clause 7.2, Table
/// 7.2-1, when notifierInfo names no MQTT-QoS; Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS 1
/// and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes at
/// QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for a
/// JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where Robot
/// only counts it
#[gtest]
#[tokio::test]
async fn e058_03_01_with_default_protocol() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker support different mqtt version
/// Clause: 5.8.6
/// Changed: stricter than Robot, a check of the MQTT version: the observer cannot see the version the broker published
/// with, but an MQTT 3.1.1 PUBLISH carries no properties, where an MQTT 5 one may, so the observer's record of the
/// message must hold no properties member; this fails a broker that ignores MQTT-Version mqtt3.1.1 and publishes MQTT
/// 5 with properties, and Robot's client, which accepts either version, checks nothing of it
/// Changed: stricter than Robot, the message must have been received with QoS 0, the default of clause 7.2, Table
/// 7.2-1, when notifierInfo names no MQTT-QoS; Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS 1
/// and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes at
/// QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for a
/// JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where Robot
/// only counts it
#[gtest]
#[tokio::test]
async fn e058_03_02_with_mqtt_3_1_1_protocol() -> Result<()> {
    let message = mqtt_notification(
        "",
        MqttConfiguration::Anonymous,
        MqttListener::Default,
        &[MqttSetting::Version(MqttVersion::V311)],
        &[],
    )
    .await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    expect_that!(message.properties, none());
    Ok(())
}

/// Pins: Check that the broker support different mqtt version
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0, the default of clause 7.2, Table
/// 7.2-1, when notifierInfo names no MQTT-QoS; Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS 1
/// and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes at
/// QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for a
/// JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where Robot
/// only counts it
#[gtest]
#[tokio::test]
async fn e058_03_03_with_mqtt_5_0_protocol() -> Result<()> {
    let message = mqtt_notification(
        "",
        MqttConfiguration::Anonymous,
        MqttListener::Default,
        &[MqttSetting::Version(MqttVersion::V50)],
        &[],
    )
    .await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}
