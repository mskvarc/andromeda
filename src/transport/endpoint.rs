//! Endpoint transport types and URI parsing.
//!
//! Extracts the transport-layer configuration from NGSI-LD subscription
//! endpoint URIs, supporting HTTP(S) and MQTT(S) schemes.

use serde_json::Value;
use std::fmt;
use url::Url;

/// MQTT protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MqttVersion {
    V311,
    V5,
}

/// MQTT quality-of-service level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttQoS {
    AtMost,
    AtLeast,
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

/// Parse an endpoint URI (with optional notifierInfo) into an [`EndpointTransport`].
pub fn parse_endpoint_transport(uri: &str, notifier_info: Option<&[Value]>) -> Result<EndpointTransport, String> {
    let parsed = Url::parse(uri).map_err(|e| format!("Invalid endpoint URI: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(EndpointTransport::Http),
        "mqtt" | "mqtts" => {
            let use_tls = parsed.scheme() == "mqtts";
            let host = parsed.host_str().ok_or("MQTT URI missing host")?.to_string();
            let default_port = if use_tls { 8883 } else { 1883 };
            let port = parsed.port().unwrap_or(default_port);
            let topic = parsed.path().trim_start_matches('/').to_string();
            if topic.is_empty() {
                return Err("MQTT URI must include a topic path".to_string());
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
                                    _ => return Err(format!("Unsupported MQTT-Version: {value}")),
                                };
                            }
                            "MQTT-QoS" => {
                                qos = match value {
                                    "0" => MqttQoS::AtMost,
                                    "1" => MqttQoS::AtLeast,
                                    "2" => MqttQoS::Exactly,
                                    _ => return Err(format!("Unsupported MQTT-QoS: {value}")),
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
        other => Err(format!("Unsupported endpoint URI scheme: {other}")),
    }
}

#[cfg(test)]
mod tests {
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
                assert_eq!(config.host, "broker.example.com");
                assert_eq!(config.port, 1883);
                assert_eq!(config.topic, "test/topic");
                assert!(!config.use_tls);
                assert_eq!(config.version, MqttVersion::V5);
                assert_eq!(config.qos, MqttQoS::AtMost);
            }
            _ => panic!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtts_default_port() {
        let result = parse_endpoint_transport("mqtts://broker.example.com/secure/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                assert_eq!(config.port, 8883);
                assert!(config.use_tls);
                assert_eq!(config.topic, "secure/topic");
            }
            _ => panic!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_default_port() {
        let result = parse_endpoint_transport("mqtt://broker.example.com/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                assert_eq!(config.port, 1883);
                assert!(!config.use_tls);
            }
            _ => panic!("Expected Mqtt transport"),
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
                assert_eq!(config.version, MqttVersion::V311);
                assert_eq!(config.qos, MqttQoS::Exactly);
            }
            _ => panic!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_qos_1() {
        let notifier_info = vec![serde_json::json!({"key": "MQTT-QoS", "value": "1"})];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info)).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                assert_eq!(config.qos, MqttQoS::AtLeast);
            }
            _ => panic!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_no_topic_rejected() {
        let result = parse_endpoint_transport("mqtt://broker:1883", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("topic"));
    }

    #[test]
    fn parse_mqtt_root_path_rejected() {
        let result = parse_endpoint_transport("mqtt://broker:1883/", None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_unsupported_scheme_rejected() {
        let result = parse_endpoint_transport("ftp://example.com/path", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported"));
    }

    #[test]
    fn parse_invalid_mqtt_version_rejected() {
        let notifier_info = vec![serde_json::json!({"key": "MQTT-Version", "value": "mqtt4.0"})];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info));
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_mqtt_qos_rejected() {
        let notifier_info = vec![serde_json::json!({"key": "MQTT-QoS", "value": "3"})];
        let result = parse_endpoint_transport("mqtt://broker:1883/topic", Some(&notifier_info));
        assert!(result.is_err());
    }

    #[test]
    fn parse_mqtt_with_credentials() {
        let result = parse_endpoint_transport("mqtt://myuser:mypass@broker:1883/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                assert_eq!(config.username.as_deref(), Some("myuser"));
                assert_eq!(config.password.as_deref(), Some("mypass"));
                assert_eq!(config.host, "broker");
            }
            _ => panic!("Expected Mqtt transport"),
        }
    }

    #[test]
    fn parse_mqtt_with_username_only() {
        let result = parse_endpoint_transport("mqtt://myuser@broker/topic", None).unwrap();
        match result {
            EndpointTransport::Mqtt(config) => {
                assert_eq!(config.username.as_deref(), Some("myuser"));
                assert!(config.password.is_none());
            }
            _ => panic!("Expected Mqtt transport"),
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
            _ => panic!("Expected Mqtt transport"),
        }
    }
}
