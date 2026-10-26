//! Why the delivery bindings refused a subscription's notification endpoint.

use thiserror::Error;

/// Why no delivery binding this broker was built with can serve an endpoint.
///
/// The bindings live in a crate this one does not depend on, so the composition
/// that owns them classifies its own refusal into this closed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EndpointRefusal {
    /// The scheme names a protocol this broker does not deliver over.
    #[error("unsupported endpoint URI scheme; expected http, https, mqtt or mqtts")]
    UnsupportedScheme,
    /// An `mqtt` endpoint named no broker host.
    #[error("MQTT URI missing host")]
    MqttHostMissing,
    /// An `mqtt` endpoint named no topic, so there is nothing to publish to.
    #[error("MQTT URI must include a topic path")]
    MqttTopicMissing,
    /// An `mqtt` endpoint's topic path is not a valid publish topic.
    #[error("MQTT URI has a topic that is not a valid publish topic")]
    MqttTopicInvalid,
    /// `notifierInfo` asked for an MQTT version this broker does not speak.
    #[error("unsupported MQTT-Version; expected mqtt5.0 or mqtt3.1.1")]
    UnsupportedMqttVersion,
    /// `notifierInfo` asked for a `QoS` level MQTT does not define.
    #[error("unsupported MQTT-QoS; expected 0, 1 or 2")]
    UnsupportedMqttQos,
}
