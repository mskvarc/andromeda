//! The relay of the MQTT messages a test's notifications publish into its stub server's journal.
//!
//! The relay subscribes, on the observer listener of each MQTT broker of the stack, to every topic below the test's
//! namespace at quality of service 2, so it receives each message at the quality it was published with. Each message
//! is posted to the stub server at `/mqtt/<host>/<topic>`, the topic starting with the port of the listener the
//! publisher used, with a JSON body naming the topic, quality, retain flag, payload length, MQTT 5 properties and the
//! payload (as JSON when it is JSON), the shape `mosquitto_sub -F %J` prints. The test then reads the messages as
//! requests the stub server received.

use rumqttc::v5::{
    AsyncClient,
    ClientError,
    Event,
    MqttOptions,
    mqttbytes::{
        QoS,
        v5::{Packet, Publish},
    },
};
use serde_json::{Map, Value as Json, json};
use std::time::Duration;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};
use url::Host;

/// Why the relay could not start.
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    /// The subscription could not be sent.
    #[error("the relay could not subscribe on {0}")]
    Subscribe(Host<String>, #[source] Box<ClientError>),
    /// The broker did not acknowledge the subscription in time.
    #[error("the MQTT broker {0} did not acknowledge the relay's subscription")]
    NotAcknowledged(Host<String>),
}

/// The longest the relay waits for a broker to acknowledge its subscription.
const SUBSCRIPTION_DEADLINE: Duration = Duration::from_secs(30);

/// One relayed broker.
#[derive(Debug)]
struct Relayed {
    /// The client, kept so the connection stays open.
    client: AsyncClient,
    /// The task receiving and posting messages.
    driver: JoinHandle<()>,
}

/// The relay of one test.
#[derive(Debug)]
pub struct MqttRelay {
    /// Each relayed broker.
    brokers: Vec<Relayed>,
}

impl MqttRelay {
    /// Subscribes to `+/<namespace>/#` on the observer listener `observer_port` of each of `hosts`, and returns once
    /// every broker acknowledged, so no message published afterwards is missed. Each message is posted below
    /// `stub_server`, the base URL the test process reaches its stub server by.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] when a broker does not acknowledge in time.
    pub async fn start(hosts: &[&Host<String>], observer_port: u16, namespace: &str, stub_server: &str) -> Result<MqttRelay, RelayError> {
        let http = reqwest::Client::new();
        let mut brokers = Vec::with_capacity(hosts.len());
        for host in hosts {
            let mut options = MqttOptions::new(format!("relay-{namespace}-{host}"), host.to_string(), observer_port);
            options.set_keep_alive(Duration::from_secs(30));
            let (client, mut event_loop) = AsyncClient::new(options, 64);
            let (acknowledged, subscribed) = oneshot::channel();
            let target = format!("{stub_server}/mqtt/{host}/");
            let poster = http.clone();
            let driver = tokio::spawn(async move {
                let mut acknowledged = Some(acknowledged);
                while let Ok(event) = event_loop.poll().await {
                    match event {
                        Event::Incoming(Packet::SubAck(_)) => {
                            if let Some(sender) = acknowledged.take() {
                                sender.send(()).ok();
                            }
                        }
                        Event::Incoming(Packet::Publish(publish)) => {
                            let topic = String::from_utf8_lossy(&publish.topic).into_owned();
                            // A lost relay shows as a message the test does not find, which fails the test.
                            poster
                                .post(format!("{target}{topic}"))
                                .header("content-type", "application/json")
                                .body(printed(&publish, &topic).to_string())
                                .send()
                                .await
                                .ok();
                        }
                        Event::Incoming(_) | Event::Outgoing(_) => {}
                    }
                }
            });
            client
                .subscribe(format!("+/{namespace}/#"), QoS::ExactlyOnce)
                .await
                // clone: the error names the host the configuration keeps
                .map_err(|error| RelayError::Subscribe((*host).clone(), Box::new(error)))?;
            match timeout(SUBSCRIPTION_DEADLINE, subscribed).await {
                Ok(Ok(())) => brokers.push(Relayed { client, driver }),
                // clone: the error names the host the configuration keeps
                Ok(Err(_)) | Err(_) => return Err(RelayError::NotAcknowledged((*host).clone())),
            }
        }
        Ok(MqttRelay { brokers })
    }
}

impl Drop for MqttRelay {
    // The connection tasks would otherwise outlive the relay.
    fn drop(&mut self) {
        for broker in &self.brokers {
            broker.driver.abort();
            broker.client.try_disconnect().ok();
        }
    }
}

/// The JSON of a message as `mosquitto_sub -F %J` prints it.
fn printed(publish: &Publish, topic: &str) -> Json {
    let payload = serde_json::from_slice::<Json>(&publish.payload).unwrap_or_else(|_| Json::String(String::from_utf8_lossy(&publish.payload).into_owned()));
    let mut properties = Map::new();
    if let Some(given) = &publish.properties {
        if let Some(kind) = &given.content_type {
            properties.insert("content-type".to_owned(), Json::String(kind.clone()));
        }
        if !given.user_properties.is_empty() {
            let pairs = given
                .user_properties
                .iter()
                .map(|(name, value)| json!({ name.as_str(): value }))
                .collect::<Vec<_>>();
            properties.insert("user-properties".to_owned(), Json::Array(pairs));
        }
    }
    let qos = match publish.qos {
        QoS::AtMostOnce => 0,
        QoS::AtLeastOnce => 1,
        QoS::ExactlyOnce => 2,
    };
    json!({
        "topic": topic,
        "qos": qos,
        "retain": i32::from(publish.retain),
        "payloadlen": publish.payload.len(),
        "mid": publish.pkid,
        "properties": properties,
        "payload": payload,
    })
}
