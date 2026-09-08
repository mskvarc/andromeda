//! What an `mqtt://` endpoint URI resolves to.

use crate::{endpoint::topic::MqttTopic, host_port::HostPort};
use secrecy::SecretString;
use url::Url;

/// MQTT protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display)]
pub enum MqttVersion {
    /// MQTT 3.1.1.
    #[strum(to_string = "v3.1.1")]
    V311,
    /// MQTT 5.
    #[strum(to_string = "v5")]
    V5,
}

/// MQTT quality-of-service level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum MqttQoS {
    /// At most once (`QoS` 0).
    #[strum(to_string = "QoS 0")]
    AtMost,
    /// At least once (`QoS` 1).
    #[strum(to_string = "QoS 1")]
    AtLeast,
    /// Exactly once (`QoS` 2).
    #[strum(to_string = "QoS 2")]
    Exactly,
}

/// MQTT broker connection and topic configuration.
///
/// The password is a [`SecretString`] rather than a `String` so that `Debug`
/// can be derived: a subscription's endpoint URI may embed broker credentials,
/// and a hand-written `Debug` that remembers to redact one field is a
/// hand-written `Debug` that forgets when a second is added.
#[derive(Debug, Clone)]
pub struct MqttEndpointConfig {
    /// The broker authority (host and port) this endpoint connects to.
    ///
    /// The port is defaulted from the scheme when the URI omits it.
    pub authority: HostPort,
    /// The topic notifications are published to.
    pub topic: MqttTopic,
    /// The endpoint URI, rebuilt from the validated parts with any credentials
    /// stripped, so an error or log line that names the endpoint identifies it
    /// without leaking the broker password the original URI may have carried.
    pub endpoint_uri: Url,
    /// Whether the connection is TLS (`mqtts`).
    pub use_tls: bool,
    /// The protocol version, from `notifierInfo` or defaulted to MQTT 5.
    pub version: MqttVersion,
    /// The quality of service, from `notifierInfo` or defaulted to `QoS` 0.
    pub qos: MqttQoS,
    /// The username from the URI's userinfo, if any.
    ///
    /// Free-form broker credential text with no structure to model, so it stays
    /// a `String`; the password beside it is a [`SecretString`].
    pub username: Option<String>,
    /// The password from the URI's userinfo, if any.
    pub password: Option<SecretString>,
}
