//! Why a connection to a notification endpoint failed.

use crate::http::transport::error_chain_text;
use rumqttc::{
    ClientError,
    ConnectionError,
    v5::{ClientError as ClientErrorV5, ConnectionError as ConnectionErrorV5},
};

/// The cause of a [`crate::error::TransportError::ConnectionFailed`].
///
/// Each variant is one way the transport failed to reach, or stay connected
/// to, an endpoint; the failures the transport libraries report are chained as
/// typed sources rather than flattened into text. The `rumqttc` errors are
/// boxed because they are several times larger than every other variant, and
/// each delivery result carries this type inline.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionFailure {
    /// The endpoint URI names no host to connect to.
    #[error("the endpoint URI names no host")]
    NoHost,

    /// The HTTP client could not send the request or read its response.
    ///
    /// `reqwest`'s own message names only the request that failed ("error
    /// sending request for url ..."); whether the connection was refused, reset,
    /// or closed before a response is said by the errors below it, so the whole
    /// chain is rendered.
    #[error("{}", error_chain_text(.0))]
    Http(#[source] reqwest::Error),

    /// The MQTT 5 connect handshake failed.
    #[error("MQTT v5 connection failed: {0}")]
    MqttV5Connect(#[source] Box<ConnectionErrorV5>),

    /// The MQTT 3.1.1 connect handshake failed.
    #[error("MQTT v3.1.1 connection failed: {0}")]
    MqttV311Connect(#[source] Box<ConnectionError>),

    /// The MQTT 5 client refused to enqueue the publish.
    #[error("MQTT v5 publish could not be enqueued: {0}")]
    MqttV5Publish(#[source] Box<ClientErrorV5>),

    /// The MQTT 3.1.1 client refused to enqueue the publish.
    #[error("MQTT v3.1.1 publish could not be enqueued: {0}")]
    MqttV311Publish(#[source] Box<ClientError>),

    /// The connection was lost after the publish was sent but before the
    /// broker acknowledged it.
    #[error("connection lost before the broker acknowledged the publish")]
    AcknowledgementLost,

    /// The broker is still inside its reconnect backoff, so no dial was
    /// attempted.
    #[error("connection unhealthy, backoff active")]
    BackoffActive,
}
