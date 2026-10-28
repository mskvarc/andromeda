//! ETSI `058_04`: a broker publishes an MQTT notification whose message is a well-formed notification, carrying the
//! receiver information of the subscription in its metadata (clause 7.2).

use crate::suite_keywords::{MqttConfiguration, MqttListener, mqtt_notification};
use googletest::prelude::*;
use ngsi_ld::{model::KeyValuePair, subscription::MqttNotification};
use rumqttc::v5::mqttbytes::QoS;
use serde_json::{Value, json};
use std::str::from_utf8;

/// Pins: Check that the broker send well formed data
/// Clause: 5.8.6
/// Changed: Check Message Contain Key is Dictionary Should Contain Key, whose argument Robot converts to a mapping: it
/// fails on a list, a null or a string, except a string holding a Python dict literal, which it evaluates; the port
/// requires the message, its metadata, its body and body.data[0] to be objects (isObject), stricter only for such a
/// string; and it reads data[0] as the first element of a list, where Robot's item access also reads the member "0" of
/// an object, stricter only for a data object, which clause 5.3.1 rules out (data is an array of entities)
/// Changed: stricter than Robot, the message must have been received with QoS 0, the default of clause 7.2, Table
/// 7.2-1, when notifierInfo names no MQTT-QoS; Robot's client subscribes at QoS 1, so it cannot tell QoS 0 from QoS 1
/// and receives a QoS 2 message as QoS 1, and `MQTTLibrary` does not return the QoS at all; the observer subscribes at
/// QoS 2, so it receives the QoS the message was published with, which `mosquitto_sub`'s %J record carries only for a
/// JSON payload, so a payload that is not JSON, which the JSON object of clause 7.2 rules out, fails here where Robot
/// only counts it
#[gtest]
#[tokio::test]
async fn e058_04_01_basic_subscription() -> Result<()> {
    let message = mqtt_notification(
        "",
        MqttConfiguration::Anonymous,
        MqttListener::Default,
        &[],
        &[("test-receiver-key", "test-receiver-info")],
    )
    .await?;
    expect_that!(message.qos, eq(QoS::AtMostOnce));

    let notification = MqttNotification::parse(from_utf8(&message.payload)?)?;
    expect_that!(
        notification.metadata().iter().map(KeyValuePair::key).collect::<Vec<_>>(),
        contains(eq(&"Content-Type"))
    );
    expect_that!(notification.link(), some(anything()));
    expect_that!(notification.receiver_info("test-receiver-key"), some(eq("test-receiver-info")));

    let body: Value = serde_json::from_str(notification.body())?;
    expect_that!(body.get("type"), some(eq(&json!("Notification"))));
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.get("data"), some(anything()));
    expect_that!(body.pointer("/data/0").and_then(Value::as_object), some(anything()));
    expect_that!(body.pointer("/data/0/id"), some(anything()));
    expect_that!(body.pointer("/data/0/type"), some(anything()));
    Ok(())
}
