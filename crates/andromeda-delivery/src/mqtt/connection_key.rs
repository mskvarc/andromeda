//! What makes two MQTT endpoints the same connection.

use crate::{
    endpoint::mqtt::{MqttEndpointConfig, MqttVersion},
    host_port::HostPort,
};
use secrecy::ExposeSecret;
use std::hash::{DefaultHasher, Hash, Hasher};

/// The identity a pooled connection is cached under.
///
/// Deliberately not the whole endpoint: the topic and the quality of service
/// are per-publish, so two subscriptions pointing at different topics on the
/// same broker share one connection. The credentials are part of the identity
/// because two subscriptions authenticating differently are two sessions, and
/// the password appears as a hash so the key can derive `Debug` and `Eq`
/// without holding the secret.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MqttConnectionKey {
    /// The broker authority (host and port).
    pub authority: HostPort,
    /// Whether the connection is TLS.
    pub use_tls: bool,
    /// The protocol version, which decides which client is built.
    pub version: MqttVersion,
    /// The username, if the endpoint named one.
    pub username: Option<String>,
    /// A hash of the password, if the endpoint named one.
    pub password_hash: Option<u64>,
}

impl MqttConnectionKey {
    /// The key an endpoint's connection is pooled under.
    #[must_use]
    pub fn from_config(config: &MqttEndpointConfig) -> MqttConnectionKey {
        let password_hash = config.password.as_ref().map(|password| {
            let mut hasher = DefaultHasher::new();
            password.expose_secret().hash(&mut hasher);
            hasher.finish()
        });
        MqttConnectionKey {
            authority: config.authority.clone(), // clone: the key outlives the endpoint it was read from
            use_tls: config.use_tls,
            version: config.version,
            username: config.username.clone(),
            password_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::{
            mqtt::MqttEndpointConfig,
            transport::{EndpointTransport, parse_endpoint_transport},
        },
        mqtt::connection_key::MqttConnectionKey,
    };

    fn endpoint(uri: &str) -> MqttEndpointConfig {
        let uri = uri.parse().expect("the URI parses");
        match parse_endpoint_transport(&uri, None).expect("the endpoint parses") {
            EndpointTransport::Mqtt(config) => config,
            EndpointTransport::Http => panic!("expected an MQTT transport for {uri}"),
        }
    }

    #[test]
    fn two_subscriptions_on_different_topics_of_one_broker_share_a_connection() {
        let first = MqttConnectionKey::from_config(&endpoint("mqtt://broker:1883/topic/a"));
        let second = MqttConnectionKey::from_config(&endpoint("mqtt://broker:1883/topic/b"));
        pretty_assertions::assert_eq!(first, second);
    }

    #[test]
    fn two_subscriptions_authenticating_differently_do_not_share_a_connection() {
        let anonymous = MqttConnectionKey::from_config(&endpoint("mqtt://broker:1883/topic"));
        let authenticated = MqttConnectionKey::from_config(&endpoint("mqtt://user:pass@broker:1883/topic"));
        let other_password = MqttConnectionKey::from_config(&endpoint("mqtt://user:other@broker:1883/topic"));

        assert_ne!(anonymous, authenticated);
        assert_ne!(authenticated, other_password);
    }

    #[test]
    fn a_key_does_not_carry_the_password_it_was_built_from() {
        let key = MqttConnectionKey::from_config(&endpoint("mqtt://user:hunter2@broker:1883/topic"));
        let rendered = format!("{key:?}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
    }
}
