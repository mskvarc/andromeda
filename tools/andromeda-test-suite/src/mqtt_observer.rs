//! The receiver of a test's MQTT notifications: an MQTT 5 client subscribed to the test's topic.
//!
//! The observer connects to the listener of a Mosquitto broker that has no mount point, where a message published
//! through the listener `<port>` appears under `<port>/<topic>`, so the topic a message arrives on names the port it
//! was published through as well as the broker. It subscribes at quality of service 2, so the quality it receives a
//! message with is the one the message was published with, and it keeps every message in arrival order with its
//! retain flag and its MQTT 5 properties (none for a message published over MQTT 3.1.1).

use crate::eventually::{Arrivals, Poll, TimedOut, delivery_deadline, eventually};
use bytes::Bytes;
use indexmap::IndexMap;
use ngsi_ld::{
    model::{DocumentError, Entity, Lossless, Output},
    subscription::{Notification, ReadErrorKind},
};
use rumqttc::v5::{
    AsyncClient,
    ClientError,
    Event,
    MqttOptions,
    mqttbytes::{
        QoS,
        v5::{Packet, PublishProperties},
    },
};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::{
    sync::{RwLock, oneshot},
    task::JoinHandle,
    time::timeout,
};

/// Why the observer could not subscribe or a message could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ObserverError {
    /// The subscription could not be sent.
    #[error("the observer could not subscribe")]
    Subscribe(#[source] Box<ClientError>),
    /// The broker did not acknowledge the subscription in time.
    #[error("the MQTT broker did not acknowledge the subscription within the delivery deadline")]
    NotAcknowledged,
    /// The awaited message did not arrive.
    #[error(transparent)]
    TimedOut(#[from] TimedOut<Arrivals>),
}

/// A message as the observer received it.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    /// The topic, behind the port it was published through.
    pub topic: Bytes,
    /// The quality of service it was published with.
    pub qos: QoS,
    /// Whether the broker delivered it as a retained message.
    pub retain: bool,
    /// Its MQTT 5 properties, none for a message published over MQTT 3.1.1.
    pub properties: Option<PublishProperties>,
    /// Its payload.
    pub payload: Bytes,
}

impl ReceivedMessage {
    /// The payload as a JSON tree.
    ///
    /// # Errors
    ///
    /// Returns the parse error when the payload is not JSON.
    pub fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }

    /// The notification the payload carries (clause 7.2): the `metadata` an HTTP notification would carry as header
    /// fields, and the `body`, read in `form`.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError`] when the payload is not that object or its body is not a notification.
    pub fn notification(&self, form: Lossless) -> Result<MqttNotification, PayloadError> {
        let payload = self.json()?;
        let (Some(serde_json::Value::Object(metadata)), Some(body)) = (payload.get("metadata"), payload.get("body")) else {
            return Err(PayloadError::NotEnvelope);
        };
        let metadata = metadata
            .iter()
            .map(|(name, value)| value.as_str().map(|text| (name.clone(), text.to_owned())).ok_or(PayloadError::NotEnvelope))
            .collect::<Result<_, _>>()?;
        Ok(MqttNotification {
            metadata,
            body: Notification::<Vec<Entity<Output>>>::parse(&body.to_string(), form)?,
        })
    }
}

/// A notification an MQTT message carries (clause 7.2).
#[derive(Debug, Clone)]
pub struct MqttNotification {
    /// The metadata: the header fields an HTTP notification would carry, by name as the broker wrote it.
    pub metadata: IndexMap<String, String>,
    /// The notification.
    pub body: Notification<Vec<Entity<Output>>>,
}

/// Why an MQTT payload is not a notification.
#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    /// The payload is not JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The payload is not an object of a `metadata` object of texts and a `body`.
    #[error("the payload is not an object of metadata and body")]
    NotEnvelope,
    /// The body is not a notification of the representation asked for.
    #[error(transparent)]
    Document(#[from] DocumentError<ReadErrorKind<Infallible>>),
}

/// An observer of one topic of one MQTT broker.
#[derive(Debug)]
pub struct MqttObserver {
    /// The client, kept so the connection stays open.
    client: AsyncClient,
    /// Every message received, in arrival order.
    messages: Arc<RwLock<Vec<ReceivedMessage>>>,
    /// The task driving the connection.
    driver: JoinHandle<()>,
}

impl MqttObserver {
    /// Subscribes to `<port>/<topic>` on the observer listener of `host`, and returns once the broker acknowledged
    /// the subscription, so no message published afterwards is missed.
    ///
    /// # Errors
    ///
    /// Returns [`ObserverError`] when the subscription is not acknowledged within ten seconds.
    pub async fn subscribe(host: &str, observer_port: u16, port: u16, topic: &str, client_id: &str) -> Result<MqttObserver, ObserverError> {
        let mut options = MqttOptions::new(client_id, host, observer_port);
        options.set_keep_alive(Duration::from_secs(30));
        let (client, mut event_loop) = AsyncClient::new(options, 64);
        let messages = Arc::new(RwLock::new(Vec::new()));
        let (acknowledged, subscribed) = oneshot::channel();
        let recorded = Arc::clone(&messages);
        let driver = tokio::spawn(async move {
            let mut acknowledged = Some(acknowledged);
            while let Ok(event) = event_loop.poll().await {
                match event {
                    Event::Incoming(Packet::SubAck(_)) => {
                        if let Some(sender) = acknowledged.take() {
                            sender.send(()).ok();
                        }
                    }
                    Event::Incoming(Packet::Publish(publish)) => recorded.write().await.push(ReceivedMessage {
                        topic: publish.topic,
                        qos: publish.qos,
                        retain: publish.retain,
                        properties: publish.properties,
                        payload: publish.payload,
                    }),
                    Event::Incoming(_) | Event::Outgoing(_) => {}
                }
            }
        });
        client
            .subscribe(format!("{port}/{topic}"), QoS::ExactlyOnce)
            .await
            .map_err(|error| ObserverError::Subscribe(Box::new(error)))?;
        match timeout(delivery_deadline(), subscribed).await {
            Ok(Ok(())) => Ok(MqttObserver { client, messages, driver }),
            Ok(Err(_)) | Err(_) => Err(ObserverError::NotAcknowledged),
        }
    }

    /// Every message received so far, oldest first.
    pub async fn received(&self) -> Vec<ReceivedMessage> {
        self.messages.read().await.clone()
    }

    /// The messages received once at least `count` have arrived, waiting up to the [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`ObserverError::TimedOut`] when fewer arrive within the deadline.
    pub async fn wait_for(&self, count: usize) -> Result<Vec<ReceivedMessage>, ObserverError> {
        Ok(eventually(delivery_deadline(), || async move {
            let received = self.received().await;
            if received.len() >= count {
                Poll::Ready(received)
            } else {
                Poll::Pending(Arrivals {
                    held: received.len(),
                    awaited: count,
                })
            }
        })
        .await?)
    }

    /// Forgets every message received so far, as a client subscribing anew starts with none.
    pub async fn clear(&self) {
        self.messages.write().await.clear();
    }
}

impl Drop for MqttObserver {
    // The connection task would otherwise outlive the observer; the client is dropped with it.
    fn drop(&mut self) {
        self.driver.abort();
        self.client.try_disconnect().ok();
    }
}

#[cfg(test)]
mod tests {
    use crate::mqtt_observer::{PayloadError, ReceivedMessage};
    use bytes::Bytes;
    use googletest::prelude::*;
    use ngsi_ld::model::Lossless;
    use rumqttc::v5::mqttbytes::QoS;

    /// A message published with `payload`.
    fn message(payload: &'static str) -> ReceivedMessage {
        ReceivedMessage {
            topic: Bytes::from_static(b"1883/t"),
            qos: QoS::AtLeastOnce,
            retain: false,
            properties: None,
            payload: Bytes::from_static(payload.as_bytes()),
        }
    }

    #[gtest]
    fn a_payload_of_metadata_and_body_reads_as_a_notification() -> Result<()> {
        let received = message(concat!(
            r#"{"metadata":{"Content-Type":"application/json"},"body":{"id":"urn:ngsi-ld:Notification:1","type":"Notification","#,
            r#""subscriptionId":"urn:ngsi-ld:Subscription:1","notifiedAt":"2024-01-01T00:00:00Z","data":[{"id":"urn:ngsi-ld:V:1","type":"V"}]}}"#
        ));
        let notification = received.notification(Lossless::Normalized)?;
        expect_that!(notification.metadata.get("Content-Type").map(String::as_str), some(eq("application/json")));
        verify_that!(notification.body.data().as_slice(), len(eq(1)))
    }

    #[gtest]
    fn a_payload_without_metadata_is_refused() -> Result<()> {
        let refused = message(r#"{"body":{}}"#).notification(Lossless::Normalized);
        verify_that!(refused.is_err_and(|error| matches!(error, PayloadError::NotEnvelope)), eq(true))
    }
}
