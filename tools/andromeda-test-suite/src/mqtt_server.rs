//! An MQTT server of the test process: the brokers publish a test's notifications to it, and the test stops it to cut
//! their connections to it.
//!
//! It speaks MQTT 5, the protocol a broker dials when a Subscription names no `MQTT-Version`. It accepts every client,
//! grants every subscription, acknowledges each publish at its quality of service and keeps every publish in arrival
//! order. It forwards nothing. The stack's Mosquitto brokers carry the other MQTT tests, so stopping one of them would
//! cut those tests' connections too, and a test that needs its server to go away serves its own.

use crate::{
    config::SuiteConfig,
    config_error::ConfigError,
    eventually::{Arrivals, Poll, TimedOut, delivery_deadline, eventually},
    front_route::callback_host,
};
use futures::{SinkExt, StreamExt};
use rumqttc::v5::mqttbytes::{
    QoS,
    v5::{Codec, ConnAck, ConnectReturnCode, Packet, PingResp, PubAck, PubComp, PubRec, Publish, SubAck, SubscribeReasonCode, UnsubAck, UnsubAckReason},
};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::RwLock,
    task::{JoinHandle, JoinSet},
};
use tokio_util::codec::Framed;
use url::{Host, Url};

/// The largest packet, in octets, the server reads or writes: 1 MiB, far above a notification a test publishes to it.
const MAX_PACKET: u32 = 1024 * 1024;

/// Why the server could not start or name an endpoint.
#[derive(Debug, thiserror::Error)]
pub enum MqttServerError {
    /// The configuration names no host the brokers reach the test process by.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The listening socket could not be opened.
    #[error("the MQTT server could not listen")]
    Listen(#[source] io::Error),
    /// The endpoint of a topic on the server is not a URL.
    #[error("the MQTT server on port {port} has no endpoint for the topic asked")]
    Endpoint {
        /// The port the server listens on.
        port: u16,
        /// Why the endpoint does not parse.
        #[source]
        source: url::ParseError,
    },
}

/// An MQTT server the brokers publish to.
#[derive(Debug)]
pub struct MqttServer {
    /// The host the brokers reach the server by.
    host: Host<String>,
    /// The port the server listens on.
    port: u16,
    /// Every publish received, in arrival order.
    publishes: Arc<RwLock<Vec<Publish>>>,
    /// The task accepting connections, which owns the listening socket and every session.
    accepting: JoinHandle<()>,
}

impl MqttServer {
    /// A server listening on every interface of the runner container, on a port of its own.
    ///
    /// # Errors
    ///
    /// Returns [`MqttServerError`] when no port can be opened or no callback host is known.
    pub async fn start() -> Result<MqttServer, MqttServerError> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .await
            .map_err(MqttServerError::Listen)?;
        let port = listener.local_addr().map_err(MqttServerError::Listen)?.port();
        let host = callback_host(SuiteConfig::get().map_err(ConfigError::clone)?)?;
        let publishes = Arc::new(RwLock::new(Vec::new()));
        let recorded = Arc::clone(&publishes);
        let accepting = tokio::spawn(async move {
            // Dropped with this task, the set aborts every session, and each session drops its connection.
            let mut sessions = JoinSet::new();
            while let Ok((connection, _)) = listener.accept().await {
                sessions.spawn(serve(connection, Arc::clone(&recorded)));
            }
        });
        Ok(MqttServer {
            host,
            port,
            publishes,
            accepting,
        })
    }

    /// The host the brokers reach the server by.
    #[must_use]
    pub const fn host(&self) -> &Host<String> {
        &self.host
    }

    /// The port the server listens on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The `mqtt` endpoint of `topic` on the server, as the brokers reach it.
    ///
    /// # Errors
    ///
    /// Returns [`MqttServerError::Endpoint`] when `topic` breaks the URL.
    pub fn endpoint(&self, topic: &str) -> Result<Url, MqttServerError> {
        Url::parse(&format!("mqtt://{}:{}/{topic}", self.host, self.port)).map_err(|source| MqttServerError::Endpoint { port: self.port, source })
    }

    /// Every publish received so far, oldest first.
    pub async fn received(&self) -> Vec<Publish> {
        self.publishes.read().await.clone()
    }

    /// The publishes received once at least `count` have arrived, waiting up to the [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`TimedOut`] when fewer arrive within the deadline.
    pub async fn wait_for(&self, count: usize) -> Result<Vec<Publish>, TimedOut<Arrivals>> {
        eventually(delivery_deadline(), || async move {
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
        .await
    }

    /// Stops the server, closing its listening socket and every connection to it, as a server that goes away does.
    pub fn stop(&self) {
        self.accepting.abort();
    }
}

impl Drop for MqttServer {
    // The accepting task would otherwise keep the port and every session open after the test.
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

/// Serves one client until it disconnects, sends what the server cannot read, or the server stops.
async fn serve(connection: TcpStream, publishes: Arc<RwLock<Vec<Publish>>>) {
    let mut session = Framed::new(
        connection,
        Codec {
            max_incoming_size: Some(MAX_PACKET),
            max_outgoing_size: Some(MAX_PACKET),
        },
    );
    while let Some(Ok(packet)) = session.next().await {
        if let Packet::Publish(publish) = &packet {
            publishes.write().await.push(publish.clone());
        }
        if let Some(answer) = answer(&packet)
            && session.send(answer).await.is_err()
        {
            return;
        }
    }
}

/// The packet the server answers `packet` with, when the protocol has it answer one.
fn answer(packet: &Packet) -> Option<Packet> {
    match packet {
        Packet::Connect(..) => Some(Packet::ConnAck(ConnAck {
            session_present: false,
            code: ConnectReturnCode::Success,
            properties: None,
        })),
        Packet::Subscribe(subscribe) => Some(Packet::SubAck(SubAck {
            pkid: subscribe.pkid,
            return_codes: subscribe.filters.iter().map(|filter| SubscribeReasonCode::Success(filter.qos)).collect(),
            properties: None,
        })),
        Packet::Unsubscribe(unsubscribe) => Some(Packet::UnsubAck(UnsubAck {
            pkid: unsubscribe.pkid,
            reasons: unsubscribe.filters.iter().map(|_| UnsubAckReason::Success).collect(),
            properties: None,
        })),
        Packet::Publish(publish) => match publish.qos {
            QoS::AtMostOnce => None,
            QoS::AtLeastOnce => Some(Packet::PubAck(PubAck::new(publish.pkid, None))),
            QoS::ExactlyOnce => Some(Packet::PubRec(PubRec::new(publish.pkid, None))),
        },
        Packet::PubRel(release) => Some(Packet::PubComp(PubComp::new(release.pkid, None))),
        Packet::PingReq(_) => Some(Packet::PingResp(PingResp)),
        Packet::Auth(_)
        | Packet::ConnAck(_)
        | Packet::PubAck(_)
        | Packet::PingResp(_)
        | Packet::SubAck(_)
        | Packet::PubRec(_)
        | Packet::PubComp(_)
        | Packet::UnsubAck(_)
        | Packet::Disconnect(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::mqtt_server::answer;
    use googletest::prelude::*;
    use rumqttc::v5::mqttbytes::{
        QoS,
        v5::{ConnAck, Connect, ConnectReturnCode, Filter, Packet, PingReq, PubAck, PubComp, PubRec, PubRel, Publish, SubAck, Subscribe, SubscribeReasonCode},
    };

    /// A publish of packet identifier 7 at `qos`.
    fn publish(qos: QoS) -> Packet {
        let mut publish = Publish::new("t", qos, "n", None);
        publish.pkid = 7;
        Packet::Publish(publish)
    }

    #[gtest]
    fn every_client_is_accepted() {
        let connect = Connect {
            keep_alive: 30,
            client_id: "c".to_owned(),
            clean_start: true,
            properties: None,
        };
        let answered = answer(&Packet::Connect(connect, None, None));
        expect_that!(
            matches!(
                answered,
                Some(Packet::ConnAck(ConnAck {
                    code: ConnectReturnCode::Success,
                    ..
                }))
            ),
            eq(true)
        );
    }

    #[gtest]
    fn a_subscription_is_granted_at_the_quality_each_filter_asks() {
        let subscribe = Subscribe::new_many([Filter::new("$SYS/a", QoS::AtMostOnce), Filter::new("t", QoS::ExactlyOnce)], None);
        let answered = answer(&Packet::Subscribe(subscribe));
        let granted = match answered {
            Some(Packet::SubAck(SubAck { return_codes, .. })) => return_codes,
            other => panic!("a subscription is answered with a SUBACK, not {other:?}"),
        };
        expect_that!(
            granted,
            elements_are![
                eq(&SubscribeReasonCode::Success(QoS::AtMostOnce)),
                eq(&SubscribeReasonCode::Success(QoS::ExactlyOnce))
            ]
        );
    }

    #[gtest]
    fn a_publish_is_acknowledged_at_its_quality_of_service() {
        expect_that!(answer(&publish(QoS::AtMostOnce)).is_none(), eq(true));
        expect_that!(
            matches!(answer(&publish(QoS::AtLeastOnce)), Some(Packet::PubAck(PubAck { pkid: 7, .. }))),
            eq(true)
        );
        expect_that!(
            matches!(answer(&publish(QoS::ExactlyOnce)), Some(Packet::PubRec(PubRec { pkid: 7, .. }))),
            eq(true)
        );
        expect_that!(
            matches!(answer(&Packet::PubRel(PubRel::new(7, None))), Some(Packet::PubComp(PubComp { pkid: 7, .. }))),
            eq(true)
        );
    }

    #[gtest]
    fn a_ping_is_answered() {
        expect_that!(matches!(answer(&Packet::PingReq(PingReq)), Some(Packet::PingResp(_))), eq(true));
    }
}
