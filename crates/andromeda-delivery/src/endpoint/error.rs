//! Why an endpoint URI could not be read.

use crate::endpoint::topic::MqttTopicError;

/// Errors that can occur when parsing an endpoint URI.
#[derive(Debug, thiserror::Error)]
pub enum EndpointParseError {
    /// The URI is not a URI.
    #[error("invalid endpoint URI: {0}")]
    InvalidUri(#[from] url::ParseError),
    /// An `mqtt` URI named no broker host.
    #[error("MQTT URI missing host")]
    MissingHost,
    /// An `mqtt` URI named no topic, so there is nothing to publish to.
    #[error("MQTT URI must include a topic path")]
    MissingTopic,
    /// An `mqtt` URI's topic path is not a valid publish topic.
    #[error("MQTT URI has an invalid topic: {0}")]
    InvalidTopic(#[from] MqttTopicError),
    /// The scheme names a protocol this broker does not deliver over.
    #[error("unsupported endpoint URI scheme; expected http, https, mqtt, mqtts, sse or ws")]
    UnsupportedScheme,
    /// `notifierInfo` asked for an MQTT version this broker does not speak.
    #[error("unsupported MQTT-Version; expected mqtt5.0 or mqtt3.1.1")]
    UnsupportedMqttVersion,
    /// `notifierInfo` asked for a `QoS` level MQTT does not define.
    #[error("unsupported MQTT-QoS; expected 0, 1 or 2")]
    UnsupportedMqttQos,
}
