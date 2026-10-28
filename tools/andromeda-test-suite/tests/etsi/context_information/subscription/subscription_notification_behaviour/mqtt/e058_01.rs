//! ETSI `058_01`: a broker publishes an MQTT notification with the user, password and port the endpoint URL names
//! (clause 5.8.6).
//!
//! Every case shares these departures from the Robot run. Start Mqtt Server runs no container: the stack's Mosquitto
//! broker of the case's configuration stands in for it (`mosquitto` for `mosquitto.conf`, anonymous clients allowed;
//! `mosquitto-auth` for `mosquitto_with_user.conf`, the suite's password file, anonymous clients refused; both
//! listening on 1883 and 8085). The topic carries the namespace, so a test sees only its own messages. Connect,
//! Subscribe at QoS 1 and Listen become an observer subscribed at QoS 2 to the topic behind the listener port the case
//! names, so a message counts only when it came through that broker and port. Listen's one-second wait for the last
//! message is a one-second wait for at least one message, then the last. Sleep is a real wait.

use crate::suite_keywords::{MqttConfiguration, MqttListener, mqtt_notification};
use googletest::prelude::*;
use rumqttc::v5::mqttbytes::QoS;

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0; Robot's client subscribes at QoS 1,
/// so it cannot tell QoS 0 from QoS 1
#[gtest]
#[tokio::test]
async fn e058_01_01_without_user_and_port() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0; Robot's client subscribes at QoS 1,
/// so it cannot tell QoS 0 from QoS 1
#[gtest]
#[tokio::test]
async fn e058_01_02_with_non_default_port() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::NonDefault, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0; Robot's client subscribes at QoS 1,
/// so it cannot tell QoS 0 from QoS 1
#[gtest]
#[tokio::test]
async fn e058_01_03_with_user() -> Result<()> {
    let message = mqtt_notification("user@", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0; Robot's client subscribes at QoS 1,
/// so it cannot tell QoS 0 from QoS 1
#[gtest]
#[tokio::test]
async fn e058_01_04_with_user_and_password() -> Result<()> {
    let message = mqtt_notification("user_with_password:password@", MqttConfiguration::WithUser, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Changed: stricter than Robot, the message must have been received with QoS 0; Robot's client subscribes at QoS 1,
/// so it cannot tell QoS 0 from QoS 1
#[gtest]
#[tokio::test]
async fn e058_01_05_with_user_password_and_non_default_port() -> Result<()> {
    let message = mqtt_notification("user_with_password:password@", MqttConfiguration::WithUser, MqttListener::NonDefault, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}
