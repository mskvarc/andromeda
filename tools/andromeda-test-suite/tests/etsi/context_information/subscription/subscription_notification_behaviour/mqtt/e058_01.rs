//! A broker publishes an MQTT notification with the user, password and port the endpoint URL names (clause 5.8.6).
//!
//! Each test uses the stack's Mosquitto broker of its configuration (`mosquitto` for `mosquitto.conf`, anonymous
//! clients allowed; `mosquitto-auth` for `mosquitto_with_user.conf`, a password file, anonymous clients refused; both
//! listening on 1883 and 8085). The topic carries the namespace, so a test sees only its own messages. An observer
//! subscribes at QoS 2 to the topic behind the listener port the test names, so a message counts only when it came
//! through that broker and port. Listening is a one-second wait for at least one message, then the last.

use crate::mqtt_notification::{MqttConfiguration, MqttListener, mqtt_notification};
use googletest::prelude::*;
use rumqttc::v5::mqttbytes::QoS;

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1
#[gtest]
#[tokio::test]
async fn e058_01_01_without_user_and_port() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1
#[gtest]
#[tokio::test]
async fn e058_01_02_with_non_default_port() -> Result<()> {
    let message = mqtt_notification("", MqttConfiguration::Anonymous, MqttListener::NonDefault, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1
#[gtest]
#[tokio::test]
async fn e058_01_03_with_user() -> Result<()> {
    let message = mqtt_notification("user@", MqttConfiguration::Anonymous, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1
#[gtest]
#[tokio::test]
async fn e058_01_04_with_user_and_password() -> Result<()> {
    let message = mqtt_notification("user_with_password:password@", MqttConfiguration::WithUser, MqttListener::Default, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}

/// Pins: Check that the broker uses the authentication information from the endpoint url
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1
#[gtest]
#[tokio::test]
async fn e058_01_05_with_user_password_and_non_default_port() -> Result<()> {
    let message = mqtt_notification("user_with_password:password@", MqttConfiguration::WithUser, MqttListener::NonDefault, &[], &[]).await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));
    Ok(())
}
