#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::time::Duration;

use reqwest::StatusCode;
use rumqttc::QoS;

use crate::helpers::{
    MqttReceiver,
    append_entity_attrs,
    building_simple_entity,
    client,
    create_entity,
    create_subscription,
    fragment_air_quality_level,
    set_endpoint_uri,
    subscription_with_endpoint,
    unique_entity_id,
    unique_subscription_id,
    with_notifier_info,
    with_receiver_info,
};

const MQTT_BROKER_HOST: &str = "mosquitto";
const MQTT_BROKER_HOST_LOCAL: &str = "127.0.0.1";
const MQTT_BROKER_PORT: u16 = 1883;
const MQTT_AUTH_BROKER_HOST: &str = "mosquitto-auth";
const MQTT_AUTH_BROKER_HOST_LOCAL: &str = "127.0.0.1";
const MQTT_AUTH_BROKER_PORT: u16 = 9883;
const TOPIC_PREFIX: &str = "andromeda-test/notifications";

fn mqtt_uri(topic: &str) -> String {
    format!("mqtt://{MQTT_BROKER_HOST}:{MQTT_BROKER_PORT}/{topic}")
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_endpoint_without_user_and_default_port_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_01_01/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_01_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_01_01");
    let mut subscription = subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic));
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let (status, _, _) = create_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.topic, topic);
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_default_qos_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_02_01/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_02_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_02_01");
    let mut subscription = subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic));
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_qos_1_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_02_02/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_02_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_02_02");
    let mut subscription = with_notifier_info(subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic)), "MQTT-QoS", "1");
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_qos_2_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_02_03/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::ExactlyOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_02_03");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_02_03");
    let mut subscription = with_notifier_info(subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic)), "MQTT-QoS", "2");
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_default_protocol_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_03_01/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_03_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_03_01");
    let mut subscription = subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic));
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_version_3_1_1_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_03_02/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_03_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_03_02");
    let mut subscription = with_notifier_info(subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic)), "MQTT-Version", "mqtt3.1.1");
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_version_5_0_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_03_03/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_03_03");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_03_03");
    let mut subscription = with_notifier_info(subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic)), "MQTT-Version", "mqtt5.0");
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_payload_contains_metadata_and_body_envelope() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_04_01/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_04_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_04_01");
    let mut subscription = with_receiver_info(
        subscription_with_endpoint(&subscription_id, &mqtt_uri(&topic)),
        "test-receiver-key",
        "test-receiver-info",
    );
    set_endpoint_uri(&mut subscription, &mqtt_uri(&topic));
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;

    assert!(message.payload.get("metadata").is_some());
    assert!(message.payload.get("body").is_some());
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_endpoint_with_non_default_port_and_credentials_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_01_02/{}", uuid::Uuid::new_v4());
    let receiver =
        MqttReceiver::connect_with_credentials(MQTT_AUTH_BROKER_HOST_LOCAL, MQTT_AUTH_BROKER_PORT, Some("user_with_password"), Some("password")).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_01_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_01_02");
    let uri = format!("mqtt://user_with_password:password@{MQTT_AUTH_BROKER_HOST}:{MQTT_AUTH_BROKER_PORT}/{topic}");
    let mut subscription = subscription_with_endpoint(&subscription_id, &uri);
    set_endpoint_uri(&mut subscription, &uri);
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_endpoint_with_username_only_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_01_03/{}", uuid::Uuid::new_v4());
    let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_01_03");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_01_03");
    let uri = format!("mqtt://user@{MQTT_BROKER_HOST}/{topic}");
    let mut subscription = subscription_with_endpoint(&subscription_id, &uri);
    set_endpoint_uri(&mut subscription, &uri);
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_endpoint_with_username_and_password_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_01_04/{}", uuid::Uuid::new_v4());
    let receiver =
        MqttReceiver::connect_with_credentials(MQTT_AUTH_BROKER_HOST_LOCAL, MQTT_AUTH_BROKER_PORT, Some("user_with_password"), Some("password")).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_01_04");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_01_04");
    let uri = format!("mqtt://user_with_password:password@{MQTT_AUTH_BROKER_HOST}:{MQTT_AUTH_BROKER_PORT}/{topic}");
    let mut subscription = subscription_with_endpoint(&subscription_id, &uri);
    set_endpoint_uri(&mut subscription, &uri);
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_mqtt_endpoint_with_username_password_and_port_delivers_message() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let topic = format!("{TOPIC_PREFIX}/058_01_05/{}", uuid::Uuid::new_v4());
    let receiver =
        MqttReceiver::connect_with_credentials(MQTT_AUTH_BROKER_HOST_LOCAL, MQTT_AUTH_BROKER_PORT, Some("user_with_password"), Some("password")).await;
    receiver.subscribe(&topic, QoS::AtLeastOnce).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:058_01_05");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("058_01_05");
    let uri = format!("mqtt://user_with_password:password@{MQTT_AUTH_BROKER_HOST}:{MQTT_AUTH_BROKER_PORT}/{topic}");
    let mut subscription = subscription_with_endpoint(&subscription_id, &uri);
    set_endpoint_uri(&mut subscription, &uri);
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let message = receiver.wait_for_message(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(message.payload["body"]["subscriptionId"], subscription_id);
    receiver.disconnect().await;
}
