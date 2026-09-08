//! Typed MQTT notification receiver with explicit failures.

use derive_more::Into;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
    time::{Instant, timeout_at},
};
use uuid::Uuid;

/// MQTT broker host name.
#[derive(Clone, Debug, Into, PartialEq, Eq)]
pub struct MqttHost(String);

impl MqttHost {
    /// Construct a typed host name from configuration text.
    pub fn new(value: impl Into<String>) -> MqttHost {
        MqttHost(value.into())
    }

    /// Borrow the host name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// MQTT topic name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MqttTopic(String);

impl MqttTopic {
    /// Construct a typed topic.
    pub fn new(value: impl Into<String>) -> MqttTopic {
        MqttTopic(value.into())
    }

    /// Borrow the topic name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Captured MQTT notification.
#[derive(Debug)]
pub struct MqttMessage {
    /// Topic on which the payload arrived.
    pub topic: MqttTopic,
    /// Exact payload bytes.
    pub payload: Vec<u8>,
}

impl MqttMessage {
    /// Decode the exact payload bytes as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`MqttReceiverError::InvalidJson`] when the captured payload bytes are not valid JSON.
    pub fn json(&self) -> Result<Value, MqttReceiverError> {
        serde_json::from_slice(&self.payload).map_err(MqttReceiverError::InvalidJson)
    }
}

/// Optional MQTT credentials.
#[derive(Clone, Debug)]
pub struct MqttCredentials {
    username: String,
    password: String,
}

impl MqttCredentials {
    /// Construct credentials supplied by the test environment.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> MqttCredentials {
        MqttCredentials {
            username: username.into(),
            password: password.into(),
        }
    }
}

/// Connected MQTT notification receiver.
pub struct MqttReceiver {
    client: AsyncClient,
    messages: Arc<Mutex<Vec<MqttMessage>>>,
    notify: Arc<Notify>,
    task: JoinHandle<()>,
}

impl MqttReceiver {
    /// Connect to an MQTT broker and begin capturing publications.
    #[must_use]
    pub fn connect(host: MqttHost, port: u16, credentials: Option<MqttCredentials>) -> MqttReceiver {
        let client_id = format!("andromeda-test-{}", Uuid::new_v4());
        let mut options = MqttOptions::new(client_id, host, port);
        options.set_keep_alive(Duration::from_secs(30));
        if let Some(credentials) = credentials {
            options.set_credentials(credentials.username, credentials.password);
        }
        let (client, mut event_loop) = AsyncClient::new(options, 10);
        let messages = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Notify::new());
        let captured = Arc::clone(&messages);
        let signal = Arc::clone(&notify);
        let task = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publication))) => {
                        captured.lock().await.push(MqttMessage {
                            topic: MqttTopic::new(publication.topic),
                            payload: publication.payload.to_vec(),
                        });
                        signal.notify_waiters();
                    }
                    Ok(Event::Incoming(_) | Event::Outgoing(_)) => {}
                    Err(_error) => break,
                }
            }
        });
        MqttReceiver {
            client,
            messages,
            notify,
            task,
        }
    }

    /// Subscribe to a typed topic.
    ///
    /// # Errors
    ///
    /// Returns [`MqttReceiverError::Client`] when the subscription request cannot be handed to the event loop,
    /// which happens once the connection to the broker has been lost.
    pub async fn subscribe(&self, topic: &MqttTopic, qos: QoS) -> Result<(), MqttReceiverError> {
        self.client.subscribe(topic.as_str(), qos).await.map_err(MqttReceiverError::Client)
    }

    /// Wait for and remove the oldest captured message.
    ///
    /// # Errors
    ///
    /// Returns [`MqttReceiverError::Timeout`] when no publication is captured before `timeout` elapses.
    pub async fn next_message(&self, timeout: Duration) -> Result<MqttMessage, MqttReceiverError> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut messages = self.messages.lock().await;
            if !messages.is_empty() {
                return Ok(messages.remove(0));
            }
            drop(messages);
            timeout_at(deadline, self.notify.notified())
                .await
                .map_err(|_| MqttReceiverError::Timeout(timeout))?;
        }
    }

    /// Disconnect and stop the capture task.
    ///
    /// # Errors
    ///
    /// Returns [`MqttReceiverError::Client`] when the disconnect request cannot be handed to the event loop,
    /// which happens once the connection to the broker has been lost.
    pub async fn shutdown(self) -> Result<(), MqttReceiverError> {
        self.client.disconnect().await.map_err(MqttReceiverError::Client)?;
        self.task.abort();
        Ok(())
    }
}

/// MQTT connection, capture, or payload failure.
#[derive(Debug, Error)]
pub enum MqttReceiverError {
    /// MQTT client operation failed.
    #[error("MQTT operation failed: {0}")]
    Client(#[source] rumqttc::ClientError),
    /// No publication arrived within the timeout.
    #[error("no MQTT publication arrived within {0:?}")]
    Timeout(Duration),
    /// A captured payload was not valid JSON.
    #[error("MQTT payload is not valid JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
}
