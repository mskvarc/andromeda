use std::time::Duration;

use crate::harness::{fixtures, mqtt_receiver::MqttReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use rumqttc::QoS;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::mqtt";

/// MQTT broker hostname as seen from the andromeda Docker container.
const MQTT_BROKER_HOST: &str = "mosquitto";
/// MQTT broker hostname as seen from the test harness on the host.
const MQTT_BROKER_HOST_LOCAL: &str = "127.0.0.1";
const MQTT_BROKER_PORT: u16 = 1883;
/// Auth broker (mosquitto-auth service) — runs on non-default port with password auth.
const MQTT_AUTH_BROKER_HOST: &str = "mosquitto-auth";
const MQTT_AUTH_BROKER_HOST_LOCAL: &str = "127.0.0.1";
const MQTT_AUTH_BROKER_PORT: u16 = 9883;
const TOPIC: &str = "andromeda-test/notifications";

fn mqtt_endpoint_uri(topic: &str) -> String {
    format!("mqtt://{MQTT_BROKER_HOST}:{MQTT_BROKER_PORT}/{topic}")
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 058_01_01: MQTT notification without user and default port ───
    trials.push(trial(&format!("{PREFIX}::058_01_01::without_user_and_port"), |ctx| async move {
        let topic = format!("{TOPIC}/058_01_01");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        // Allow subscribe to propagate
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_01_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        assert_eq!(msg.topic, topic);
        assert!(msg.payload.get("body").is_some(), "MQTT message should have body");
        assert!(msg.payload.get("metadata").is_some(), "MQTT message should have metadata");
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_02_01: MQTT notification with default QoS ───────────────
    trials.push(trial(&format!("{PREFIX}::058_02_01::default_qos"), |ctx| async move {
        let topic = format!("{TOPIC}/058_02_01");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_02_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_02_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_02_02: MQTT notification with QoS 1 ─────────────────────
    trials.push(trial(&format!("{PREFIX}::058_02_02::qos_1"), |ctx| async move {
        let topic = format!("{TOPIC}/058_02_02");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_02_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_02_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        // Set QoS 1 via notifierInfo
        sub["notification"]["endpoint"]["notifierInfo"] = serde_json::json!([{"key": "MQTT-QoS", "value": "1"}]);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_02_03: MQTT notification with QoS 2 ─────────────────────
    trials.push(trial(&format!("{PREFIX}::058_02_03::qos_2"), |ctx| async move {
        let topic = format!("{TOPIC}/058_02_03");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::ExactlyOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_02_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_02_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        sub["notification"]["endpoint"]["notifierInfo"] = serde_json::json!([{"key": "MQTT-QoS", "value": "2"}]);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_03_01: MQTT notification with default protocol version ──
    trials.push(trial(&format!("{PREFIX}::058_03_01::default_protocol"), |ctx| async move {
        let topic = format!("{TOPIC}/058_03_01");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_03_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_03_02: MQTT notification with MQTT 3.1.1 ────────────────
    trials.push(trial(&format!("{PREFIX}::058_03_02::mqtt_311"), |ctx| async move {
        let topic = format!("{TOPIC}/058_03_02");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_03_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_03_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        sub["notification"]["endpoint"]["notifierInfo"] = serde_json::json!([{"key": "MQTT-Version", "value": "mqtt3.1.1"}]);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_03_03: MQTT notification with MQTT 5.0 ──────────────────
    trials.push(trial(&format!("{PREFIX}::058_03_03::mqtt_50"), |ctx| async move {
        let topic = format!("{TOPIC}/058_03_03");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_03_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_03_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        sub["notification"]["endpoint"]["notifierInfo"] = serde_json::json!([{"key": "MQTT-Version", "value": "mqtt5.0"}]);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_04_01: MQTT notification payload structure ──────────────
    trials.push(trial(&format!("{PREFIX}::058_04_01::payload_structure"), |ctx| async move {
        let topic = format!("{TOPIC}/058_04_01");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_04_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &mqtt_endpoint_uri(&topic));
        // Add receiverInfo to verify it appears in metadata
        sub["notification"]["endpoint"]["receiverInfo"] = serde_json::json!([{"key": "test-receiver-key", "value": "test-receiver-info"}]);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;

        // Validate envelope structure: {"metadata": {...}, "body": {...}}
        let metadata = msg.payload.get("metadata").expect("should have metadata");
        assert!(metadata.is_object(), "metadata should be an object");
        assert!(metadata.get("Content-Type").is_some(), "metadata should have Content-Type");
        assert!(metadata.get("Link").is_some(), "metadata should have Link header");
        assert_eq!(metadata["test-receiver-key"], "test-receiver-info", "metadata should contain receiverInfo");

        let body = msg.payload.get("body").expect("should have body");
        assert_eq!(body["type"], "Notification");
        assert_eq!(body["subscriptionId"], sub_id);
        let data = body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["id"], entity_id);

        receiver.disconnect().await;
    }));

    // ── 058_01_02: MQTT notification with non-default port ───────────
    // Uses the mosquitto-auth service which listens on port 9883.
    trials.push(trial(&format!("{PREFIX}::058_01_02::non_default_port"), |ctx| async move {
        let topic = format!("{TOPIC}/058_01_02");
        // Connect to auth broker anonymously — it requires auth, but we only need to subscribe
        // as the test receiver. The andromeda container connects to the broker directly.
        let receiver =
            MqttReceiver::connect_with_credentials(MQTT_AUTH_BROKER_HOST_LOCAL, MQTT_AUTH_BROKER_PORT, Some("user_with_password"), Some("password")).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_01_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_01_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        // Andromeda sees the auth broker at mosquitto-auth:9883
        set_endpoint_uri(
            &mut sub,
            &format!("mqtt://user_with_password:password@{MQTT_AUTH_BROKER_HOST}:{MQTT_AUTH_BROKER_PORT}/{topic}"),
        );
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        assert_eq!(msg.topic, topic);
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_01_03: MQTT notification with username ───────────────────
    // The anonymous broker accepts any credentials, so username-only works on the default broker.
    trials.push(trial(&format!("{PREFIX}::058_01_03::with_user"), |ctx| async move {
        let topic = format!("{TOPIC}/058_01_03");
        let receiver = MqttReceiver::connect(MQTT_BROKER_HOST_LOCAL, MQTT_BROKER_PORT).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_01_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_01_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &format!("mqtt://user@{MQTT_BROKER_HOST}/{topic}"));
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        assert_eq!(msg.topic, topic);
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_01_04: MQTT notification with username and password ──────
    // Uses the auth broker which requires credentials.
    trials.push(trial(&format!("{PREFIX}::058_01_04::with_user_and_password"), |ctx| async move {
        let topic = format!("{TOPIC}/058_01_04");
        let receiver =
            MqttReceiver::connect_with_credentials(MQTT_AUTH_BROKER_HOST_LOCAL, MQTT_AUTH_BROKER_PORT, Some("user_with_password"), Some("password")).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_01_04");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_01_04:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(
            &mut sub,
            &format!("mqtt://user_with_password:password@{MQTT_AUTH_BROKER_HOST}:{MQTT_AUTH_BROKER_PORT}/{topic}"),
        );
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        assert_eq!(msg.topic, topic);
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));

    // ── 058_01_05: MQTT notification with user, password, and non-default port
    // Same auth broker, exercises the full URI with all components.
    trials.push(trial(&format!("{PREFIX}::058_01_05::with_user_password_and_port"), |ctx| async move {
        let topic = format!("{TOPIC}/058_01_05");
        let receiver =
            MqttReceiver::connect_with_credentials(MQTT_AUTH_BROKER_HOST_LOCAL, MQTT_AUTH_BROKER_PORT, Some("user_with_password"), Some("password")).await;
        receiver.subscribe(&topic, QoS::AtLeastOnce).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:058_01_05");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:058_01_05:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(
            &mut sub,
            &format!("mqtt://user_with_password:password@{MQTT_AUTH_BROKER_HOST}:{MQTT_AUTH_BROKER_PORT}/{topic}"),
        );
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let msg = receiver.wait_for_message(Duration::from_secs(5)).await;
        assert_eq!(msg.topic, topic);
        let body = &msg.payload["body"];
        assert_eq!(body["subscriptionId"], sub_id);

        receiver.disconnect().await;
    }));
}
