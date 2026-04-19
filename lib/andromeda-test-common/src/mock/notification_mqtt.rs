//! MQTT notification receiver.
//!
//! Migrated from `tests/notification/helpers.rs:635-713`. MQTT tests stay
//! `#[serial(mqtt)]` because the MQTT broker shares a topic namespace
//! across tests.

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

/// One captured MQTT message.
pub struct MqttMessage {
    /// Topic the message was published to.
    pub topic: String,
    /// Parsed JSON payload; `Value::Null` when the body was empty or not JSON.
    pub payload: Value,
}

/// Connected MQTT consumer. Subscribes to topics via [`MqttReceiver::subscribe`]
/// and buffers every incoming publish for later inspection.
pub struct MqttReceiver {
    client: AsyncClient,
    messages: Arc<Mutex<Vec<MqttMessage>>>,
    notify: Arc<Notify>,
    _event_loop: tokio::task::JoinHandle<()>,
}

impl MqttReceiver {
    /// Connect without authentication.
    pub async fn connect(host: &str, port: u16) -> MqttReceiver {
        MqttReceiver::connect_with_credentials(host, port, None, None).await
    }

    /// Connect with optional credentials.
    pub async fn connect_with_credentials(host: &str, port: u16, username: Option<&str>, password: Option<&str>) -> MqttReceiver {
        let client_id = format!("notification-test-receiver-{}", Uuid::new_v4());
        let mut options = MqttOptions::new(client_id, host, port);
        options.set_keep_alive(Duration::from_secs(30));
        if let Some(username) = username {
            options.set_credentials(username, password.unwrap_or(""));
        }

        let (client, mut event_loop) = AsyncClient::new(options, 10);
        let messages = Arc::new(Mutex::new(Vec::<MqttMessage>::new()));
        let notify = Arc::new(Notify::new());
        let message_store = messages.clone();
        let signal = notify.clone();

        let event_loop_handle = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let payload = serde_json::from_slice::<Value>(&publish.payload).unwrap_or(Value::Null);
                        message_store.lock().await.push(MqttMessage { topic: publish.topic, payload });
                        signal.notify_one();
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        MqttReceiver {
            client,
            messages,
            notify,
            _event_loop: event_loop_handle,
        }
    }

    /// Subscribe to `topic`.
    pub async fn subscribe(&self, topic: &str, qos: QoS) {
        if let Err(error) = self.client.subscribe(topic, qos).await {
            eprintln!("MqttReceiver subscribe({topic}) failed: {error}");
            std::process::exit(2);
        }
    }

    /// Wait up to `timeout` for a message, pop and return it.
    ///
    /// Exits with status 2 if the timeout is exceeded.
    pub async fn wait_for_message(&self, timeout: Duration) -> MqttMessage {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut messages = self.messages.lock().await;
                if !messages.is_empty() {
                    return messages.remove(0);
                }
            }
            match tokio::time::timeout_at(deadline, self.notify.notified()).await {
                Ok(_) => {}
                Err(_) => {
                    eprintln!("timed out waiting for mqtt message after {timeout:?}");
                    std::process::exit(2);
                }
            }
        }
    }

    /// Close the MQTT connection.
    pub async fn disconnect(self) {
        let _ = self.client.disconnect().await;
    }
}
