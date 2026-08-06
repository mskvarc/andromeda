//! Why an endpoint URI could not be read.

/// Errors that can occur when parsing an endpoint URI.
#[derive(Debug, thiserror::Error)]
pub enum EndpointParseError {
    /// The URI is not a URI.
    #[error("invalid endpoint URI: {source}")]
    InvalidUri {
        /// The underlying parse failure.
        #[from]
        source: url::ParseError,
    },
    /// An `mqtt` URI named no broker host.
    #[error("MQTT URI missing host")]
    MissingHost,
    /// An `mqtt` URI named no topic, so there is nothing to publish to.
    #[error("MQTT URI must include a topic path")]
    MissingTopic,
    /// The scheme names a protocol this broker does not deliver over.
    #[error("unsupported endpoint URI scheme: {scheme}")]
    UnsupportedScheme {
        /// The scheme that was given.
        scheme: String,
    },
    /// `notifierInfo` asked for an MQTT version this broker does not speak.
    #[error("unsupported MQTT-Version: {value}")]
    UnsupportedMqttVersion {
        /// The value that was given.
        value: String,
    },
    /// `notifierInfo` asked for a QoS level MQTT does not define.
    #[error("unsupported MQTT-QoS: {value}")]
    UnsupportedMqttQos {
        /// The value that was given.
        value: String,
    },
}
