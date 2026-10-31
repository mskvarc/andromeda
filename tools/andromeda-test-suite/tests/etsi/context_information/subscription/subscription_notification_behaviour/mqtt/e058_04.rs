//! A broker publishes an MQTT notification whose message is a well-formed notification, carrying the receiver
//! information of the subscription in its metadata (clause 7.2).

use crate::mqtt_notification::{MqttConfiguration, MqttListener, mqtt_notification};
use googletest::prelude::*;
use ngsi_ld::{model::KeyValuePair, subscription::MqttNotification};
use rumqttc::v5::mqttbytes::QoS;
use serde_json::{Value, json};
use std::str::from_utf8;

/// Pins: Check that the broker send well formed data
/// Clause: 5.8.6
/// Expected: the message arrives with QoS 0, the default of clause 7.2, Table 7.2-1, when notifierInfo names no
/// MQTT-QoS; the observer subscribes at QoS 2, so it receives the QoS the message was published with, and records it
/// only for a JSON payload, which clause 7.2 requires
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
