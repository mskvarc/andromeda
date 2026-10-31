//! A broker publishes an MQTT notification with the protocol version that the subscription's `notifierInfo` names
//! (clause 5.8.6).

use crate::mqtt_notification::{MqttConfiguration, MqttListener, MqttSetting, MqttVersion, mqtt_notification};
use googletest::prelude::*;
use rumqttc::v5::mqttbytes::QoS;

/// Pins: Check that the broker support different mqtt version
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1, when notifierInfo names no
/// MQTT-QoS; the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it
/// only for a JSON payload, which clause 7.2 requires
#[gtest]
#[tokio::test]
async fn e058_03_01_with_default_protocol() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker support different mqtt version
/// Clause: 5.8.6
/// Expected: an MQTT 3.1.1 PUBLISH carries no properties, where an MQTT 5 one may, so the observer's record of the
/// message holds no properties member; a broker that ignores MQTT-Version mqtt3.1.1 and publishes MQTT 5 with
/// properties fails
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1, when notifierInfo names no
/// MQTT-QoS; the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it
/// only for a JSON payload, which clause 7.2 requires
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
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1, when notifierInfo names no
/// MQTT-QoS; the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it
/// only for a JSON payload, which clause 7.2 requires
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
