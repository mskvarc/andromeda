use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::Mutex;

pub struct MqttMessage {
    pub topic: String,
    pub payload: serde_json::Value,
}

pub struct MqttReceiver {
    client: AsyncClient,
    messages: std::sync::Arc<Mutex<Vec<MqttMessage>>>,
    notify: std::sync::Arc<tokio::sync::Notify>,
    _event_loop: tokio::task::JoinHandle<()>,
}

impl MqttReceiver {
    pub async fn connect(host: &str, port: u16) -> Self {
        Self::connect_with_credentials(host, port, None, None).await
    }

    pub async fn connect_with_credentials(host: &str, port: u16, username: Option<&str>, password: Option<&str>) -> Self {
        let client_id = format!("test-receiver-{}", uuid::Uuid::new_v4());
        let mut opts = MqttOptions::new(client_id, host, port);
        opts.set_keep_alive(Duration::from_secs(30));
        if let Some(user) = username {
            opts.set_credentials(user, password.unwrap_or(""));
        }

        let (client, mut eventloop) = AsyncClient::new(opts, 10);

        let messages: std::sync::Arc<Mutex<Vec<MqttMessage>>> = std::sync::Arc::new(Mutex::new(Vec::new()));
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());

        let msgs = messages.clone();
        let ntf = notify.clone();

        let handle = tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let payload: serde_json::Value = serde_json::from_slice(&publish.payload).unwrap_or(serde_json::Value::Null);
                        msgs.lock().await.push(MqttMessage {
                            topic: publish.topic.clone(),
                            payload,
                        });
                        ntf.notify_one();
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Self {
            client,
            messages,
            notify,
            _event_loop: handle,
        }
    }

    pub async fn subscribe(&self, topic: &str, qos: QoS) {
        self.client.subscribe(topic, qos).await.expect("MQTT subscribe failed");
    }

    pub async fn wait_for_message(&self, timeout: Duration) -> MqttMessage {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut msgs = self.messages.lock().await;
                if !msgs.is_empty() {
                    return msgs.remove(0);
                }
            }
            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                panic!("Timed out waiting for MQTT message after {:?}", timeout);
            }
        }
    }

    pub async fn disconnect(self) {
        let _ = self.client.disconnect().await;
    }
}
