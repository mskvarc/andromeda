//! A broker publishes an MQTT notification at the quality of service the subscription's `notifierInfo` names (clause
//! 5.8.6).

use crate::mqtt_notification::{MqttConfiguration, MqttListener, MqttSetting, mqtt_notification};
use googletest::prelude::*;
use rumqttc::v5::mqttbytes::QoS;

/// Pins: Check that the broker supports different quality of service (qos)
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1, when notifierInfo names no
/// MQTT-QoS; the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it
/// only for a JSON payload, which clause 7.2 requires
#[gtest]
#[tokio::test]
async fn e058_02_01_with_default_qos() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker supports different quality of service (qos)
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 1, the MQTT-QoS the subscription's notifierInfo names (clause 7.2, Table
/// 7.2-1); the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it only
/// for a JSON payload, which clause 7.2 requires
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
/// Expected: the message arrives with QoS 2, the MQTT-QoS the subscription's notifierInfo names (clause 7.2, Table
/// 7.2-1); the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it only
/// for a JSON payload, which clause 7.2 requires
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
