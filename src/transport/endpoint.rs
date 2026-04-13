//! Endpoint transport types and URI parsing.
//!
//! Extracts the transport-layer configuration from NGSI-LD subscription
//! endpoint URIs, supporting HTTP(S) and MQTT(S) schemes.

use serde_json::Value;
use std::fmt;
use url::Url;

/// MQTT protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display)]
pub enum MqttVersion {
    #[strum(to_string = "v3.1.1")]
    V311,
    #[strum(to_string = "v5")]
    V5,
}

/// MQTT quality-of-service level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum MqttQoS {
    #[strum(to_string = "QoS 0")]
    AtMost,
    #[strum(to_string = "QoS 1")]
    AtLeast,
    #[strum(to_string = "QoS 2")]
    Exactly,
}

/// MQTT broker connection and topic configuration.
#[derive(Clone)]
pub struct MqttEndpointConfig {
    pub host: String,
    pub port: u16,
    pub topic: String,
    pub use_tls: bool,
    pub version: MqttVersion,
    pub qos: MqttQoS,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl fmt::Debug for MqttEndpointConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MqttEndpointConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("topic", &self.topic)
            .field("use_tls", &self.use_tls)
            .field("version", &self.version)
            .field("qos", &self.qos)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Notification delivery transport.
#[derive(Debug, Clone)]
pub enum EndpointTransport {
    Http,
    Mqtt(MqttEndpointConfig),
}

/// Errors that can occur when parsing an endpoint URI.
#[derive(Debug, thiserror::Error)]
pub enum EndpointParseError {
    #[error("invalid endpoint URI: {source}")]
    InvalidUri {
        #[from]
        source: url::ParseError,
    },
    #[error("MQTT URI missing host")]
    MissingHost,
    #[error("MQTT URI must include a topic path")]
    MissingTopic,
    #[error("unsupported endpoint URI scheme: {scheme}")]
    UnsupportedScheme { scheme: String },
    #[error("unsupported MQTT-Version: {value}")]
    UnsupportedMqttVersion { value: String },
    #[error("unsupported MQTT-QoS: {value}")]
    UnsupportedMqttQos { value: String },
}

/// Extract key-value pairs from an NGSI-LD `receiverInfo` or `notifierInfo`
/// JSON array into a flat `Vec<(String, String)>`.
pub fn extract_key_value_pairs(infos: Option<&[Value]>) -> Vec<(String, String)> {
    let Some(infos) = infos else {
        return Vec::new();
    };
    infos
        .iter()
        .filter_map(|info| {
            let key = info.get("key")?.as_str()?;
            let value = info.get("value")?.as_str()?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

/// Parse an endpoint URI (with optional notifierInfo) into an [`EndpointTransport`].
pub fn parse_endpoint_transport(uri: &str, notifier_info: Option<&[Value]>) -> Result<EndpointTransport, EndpointParseError> {
    let parsed = Url::parse(uri)?;
    match parsed.scheme() {
        "http" | "https" => Ok(EndpointTransport::Http),
        "mqtt" | "mqtts" => {
            let use_tls = parsed.scheme() == "mqtts";
            let host = parsed.host_str().ok_or(EndpointParseError::MissingHost)?.to_string();
            let default_port = if use_tls { 8883 } else { 1883 };
            let port = parsed.port().unwrap_or(default_port);
            let topic = parsed.path().trim_start_matches('/').to_string();
            if topic.is_empty() {
                return Err(EndpointParseError::MissingTopic);
            }

            let username = if !parsed.username().is_empty() {
                Some(parsed.username().to_string())
            } else {
                None
            };
            let password = parsed.password().map(String::from);

            let mut version = MqttVersion::V5;
            let mut qos = MqttQoS::AtMost;

            if let Some(infos) = notifier_info {
                for info in infos {
                    if let (Some(key), Some(value)) = (info.get("key").and_then(|v| v.as_str()), info.get("value").and_then(|v| v.as_str())) {
                        match key {
                            "MQTT-Version" => {
                                version = match value {
                                    "mqtt5.0" => MqttVersion::V5,
                                    "mqtt3.1.1" => MqttVersion::V311,
                                    _ => {
                                        return Err(EndpointParseError::UnsupportedMqttVersion { value: value.to_string() });
                                    }
                                };
                            }
                            "MQTT-QoS" => {
                                qos = match value {
                                    "0" => MqttQoS::AtMost,
                                    "1" => MqttQoS::AtLeast,
                                    "2" => MqttQoS::Exactly,
                                    _ => {
                                        return Err(EndpointParseError::UnsupportedMqttQos { value: value.to_string() });
                                    }
                                };
                            }
                            _ => {}
                        }
                    }
                }
            }

            Ok(EndpointTransport::Mqtt(MqttEndpointConfig {
                host,
                port,
                topic,
                use_tls,
                version,
                qos,
                username,
                password,
            }))
        }
        other => Err(EndpointParseError::UnsupportedScheme { scheme: other.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_http_endpoint() {
        let result = parse_endpoint_transport("http://example.com/notify", None).unwrap();
        assert!(matches!(result, EndpointTransport::Http));
    }

    #[test]
    fn parse_https_endpoint() {
        let result = parse_endpoint_transport("https://example.com/notify", None).unwrap();
        assert!(matches!(result, EndpointTransport::Http));
    }

    #[test]
    fn parse_mqtt_basic() {
        let result = parse_endpoint_transport("mqtt://broker.example.com:1883/test/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.host, "broker.example.com");
                pretty_assertions::assert_eq!(config.port, 1883);
                pretty_assertions::assert_eq!(config.topic, "test/topic");
                assert!(!config.use_tls);
                pretty_assertions::assert_eq!(config.version, MqttVersion::V5);
                pretty_assertions::assert_eq!(config.qos, MqttQoS::AtMost);
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtts_default_port() {
        let result = parse_endpoint_transport("mqtts://broker.example.com/secure/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.port, 8883);
                assert!(config.use_tls);
                pretty_assertions::assert_eq!(config.topic, "secure/topic");
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_default_port() {
        let result = parse_endpoint_transport("mqtt://broker.example.com/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.port, 1883);
                assert!(!config.use_tls);
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_with_notifier_info() {
        let notifier_info = vec![
            serde_json::json!({"key": "MQTT-Version", "value": "mqtt3.1.1"}),
            serde_json::json!({"key": "MQTT-QoS", "value": "2"}),
        ];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info)).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.version, MqttVersion::V311);
                pretty_assertions::assert_eq!(config.qos, MqttQoS::Exactly);
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_qos_1() {
        let notifier_info = vec![serde_json::json!({"key": "MQTT-QoS", "value": "1"})];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info)).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.qos, MqttQoS::AtLeast);
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_no_topic_rejected() {
        let result = parse_endpoint_transport("mqtt://broker:1883", None);
        assert!(matches!(result, Err(EndpointParseError::MissingTopic | EndpointParseError::InvalidUri { .. })));
    }

    #[test]
    fn parse_mqtt_root_path_rejected() {
        let result = parse_endpoint_transport("mqtt://broker:1883/", None);
        assert!(matches!(result, Err(EndpointParseError::MissingTopic)));
    }

    #[test]
    fn parse_unsupported_scheme_rejected() {
        let result = parse_endpoint_transport("ftp://example.com/path", None);
        assert!(matches!(result, Err(EndpointParseError::UnsupportedScheme { .. })));
    }

    #[test]
    fn parse_invalid_mqtt_version_rejected() {
        let notifier_info = vec![serde_json::json!({"key": "MQTT-Version", "value": "mqtt4.0"})];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info));
        assert!(matches!(result, Err(EndpointParseError::UnsupportedMqttVersion { .. })));
    }

    #[test]
    fn parse_invalid_mqtt_qos_rejected() {
        let notifier_info = vec![serde_json::json!({"key": "MQTT-QoS", "value": "3"})];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info));
        assert!(matches!(result, Err(EndpointParseError::UnsupportedMqttQos { .. })));
    }

    #[test]
    fn parse_mqtt_with_credentials() {
        let result = parse_endpoint_transport("mqtt://myuser:mypass@broker:1883/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.username.as_deref(), Some("myuser"));
                pretty_assertions::assert_eq!(config.password.as_deref(), Some("mypass"));
                pretty_assertions::assert_eq!(config.host, "broker");
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_with_username_only() {
        let result = parse_endpoint_transport("mqtt://myuser@broker/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                pretty_assertions::assert_eq!(config.username.as_deref(), Some("myuser"));
                assert!(config.password.is_none());
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_no_credentials() {
        let result = parse_endpoint_transport("mqtt://broker/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                assert!(config.username.is_none());
                assert!(config.password.is_none());
            }
            EndpointTransport::Http => unreachable!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn extract_key_value_pairs_from_receiver_info() {
        let infos = vec![
            serde_json::json!({"key": "Authorization", "value": "Bearer token"}),
            serde_json::json!({"key": "X-Custom", "value": "custom-value"}),
        ];
        let pairs = extract_key_value_pairs(Some(&infos));
        pretty_assertions::assert_eq!(pairs.len(), 2);
        pretty_assertions::assert_eq!(pairs[0], ("Authorization".to_string(), "Bearer token".to_string()));
        pretty_assertions::assert_eq!(pairs[1], ("X-Custom".to_string(), "custom-value".to_string()));
    }

    #[test]
    fn extract_key_value_pairs_none() {
        let pairs = extract_key_value_pairs(None);
        assert!(pairs.is_empty());
    }

    #[test]
    fn extract_key_value_pairs_skips_malformed() {
        let infos = vec![
            serde_json::json!({"key": "Good", "value": "pair"}),
            serde_json::json!({"key": "missing-value"}),
            serde_json::json!({"value": "missing-key"}),
            serde_json::json!("not-an-object"),
        ];
        let pairs = extract_key_value_pairs(Some(&infos));
        pretty_assertions::assert_eq!(pairs.len(), 1);
        pretty_assertions::assert_eq!(pairs[0], ("Good".to_string(), "pair".to_string()));
    }
}
