//! The `@context` of an MQTT notification (study D13).
//!
//! Clause 7.2 carries an MQTT notification's headers inside its payload, and
//! the body is the notification an HTTP receiver would get, so an
//! `application/ld+json` MQTT notification names its context under the same
//! rule, on the Notification and on every Entity.

use crate::{
    core_contexts::core,
    ngsi_requests::create_subscription,
    notification_context::create_notified_device,
    polling::eventually,
    private_tenant::PrivateTenant,
    user_contexts::{DEVICE_IRI, UserContexts},
};
use andromeda_test_mocks::{
    mqtt_receiver::{MqttHost, MqttReceiver, MqttTopic},
    server_config::MockServerConfig,
};
use reqwest::StatusCode;
use rumqttc::QoS;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

/// The port of the tier's MQTT broker.
const MQTT_PORT: u16 = 1883;

/// How long one probe waits for a message before triggering another.
const PROBE_WAIT: Duration = Duration::from_millis(500);

/// An ld+json MQTT notification names the subscription's `jsonldContext`
/// followed by the core `@context`, in its body and on every Entity.
#[tokio::test]
async fn an_mqtt_notification_names_the_context_under_the_same_rule() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let host = MockServerConfig::from_environment()
        .expect("valid callback configuration")
        .visible_host()
        .to_string();
    let topic = format!("andromeda-test/jsonld-context/{}", Uuid::new_v4());
    let receiver = MqttReceiver::connect(MqttHost::new("127.0.0.1"), MQTT_PORT, None);
    receiver
        .subscribe(&MqttTopic::new(topic.clone()), QoS::AtLeastOnce)
        .await
        .expect("the topic is subscribed"); // clone: the endpoint names the topic too
    let (status, _) = create_subscription(
        tenant.client(),
        &json!({
            "type": "Subscription",
            "entities": [{ "type": DEVICE_IRI }],
            "notificationTrigger": ["entityCreated"],
            "notification": { "endpoint": { "uri": format!("mqtt://{host}:{MQTT_PORT}/{topic}"), "accept": "application/ld+json" } },
            "jsonldContext": contexts.terms,
            "@context": core()
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The receiver's subscription is confirmed asynchronously, so a Device is
    // created until one notification arrives rather than after a fixed wait.
    let client = tenant.client();
    let receiving = &receiver;
    let payload: Value = eventually("an MQTT notification arrives", move || async move {
        create_notified_device(client).await;
        receiving
            .next_message(PROBE_WAIT)
            .await
            .ok()
            .map(|message| message.json().expect("a JSON payload"))
    })
    .await;

    let expected = json!([contexts.terms, core()]);
    assert_eq!(payload["body"]["@context"], expected, "{payload}");
    assert_eq!(payload["body"]["data"][0]["@context"], expected, "{payload}");
    assert_eq!(payload["metadata"]["Content-Type"], "application/ld+json", "{payload}");
    receiver.shutdown().await.expect("receiver shutdown");
    contexts.shutdown().await;
    tenant.finish().await;
}
