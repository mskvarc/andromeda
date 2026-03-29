use crate::query::q::Expr;
use chrono::{DateTime, Utc};
use ngsi_ld_types::subscription::SubscriptionStatus;
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MqttVersion {
    V311,
    V5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttQoS {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum EndpointTransport {
    Http,
    Mqtt(MqttEndpointConfig),
}

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
            let mut qos = MqttQoS::AtMostOnce;

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
                                    "0" => MqttQoS::AtMostOnce,
                                    "1" => MqttQoS::AtLeastOnce,
                                    "2" => MqttQoS::ExactlyOnce,
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

/// Signals subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    Upserted(String), // subscription id
    Deleted(String),  // subscription id
}

/// Signals CSR subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    Upserted(String),
    Deleted(String),
}

/// Pre-compiled entity selector for fast matching.
#[derive(Debug)]
pub struct CompiledSelector {
    pub id: Option<String>,
    pub id_pattern: Option<Regex>,
    pub type_: String,               // expanded IRI
    pub type_pattern: Option<Regex>, // regex for alternation patterns like (Building|Tower)
}

/// Pre-compiled CSR subscription for in-memory matching against CSR changes.
#[derive(Debug)]
pub struct CompiledCsrSubscription {
    pub id: String,
    pub status: SubscriptionStatus,
    pub endpoint_uri: String,
    pub endpoint_accept: Option<String>,
    pub endpoint_receiver_info: Option<Vec<Value>>,
    pub endpoint_transport: EndpointTransport,
    pub selectors: Vec<CompiledSelector>,
    pub watched_attrs: Option<HashSet<String>>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub jsonld_context: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_csf: Option<Expr>,
}

/// Pre-compiled subscription for in-memory matching.
#[derive(Debug)]
pub struct CompiledSubscription {
    pub id: String,
    pub status: SubscriptionStatus,
    pub endpoint_uri: String,
    pub endpoint_accept: Option<String>,
    pub endpoint_receiver_info: Option<Vec<Value>>,
    pub endpoint_transport: EndpointTransport,
    pub triggers: Vec<String>,
    pub watched_attrs: Option<HashSet<String>>, // expanded IRIs; None = all
    pub selectors: Vec<CompiledSelector>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub notification_attributes: Option<Vec<String>>,
    pub notification_format: Option<String>,
    pub notification_sys_attrs: bool,
    pub notification_show_changes: bool,
    pub jsonld_context: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_q: Option<Expr>,
    pub geo_q: Option<Value>,
    pub scope_q: Option<String>,
    pub dataset_id: Option<Vec<String>>,
    pub notification_join: Option<String>,
    pub notification_join_level: u32,
    pub compiled_csf: Option<Expr>,
    /// When true, only match locally stored entities, not those from CSRs (§5.5.13).
    pub local_only: bool,
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
                assert_eq!(config.qos, MqttQoS::AtMostOnce);
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
                assert_eq!(config.qos, MqttQoS::ExactlyOnce);
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
                assert_eq!(config.qos, MqttQoS::AtLeastOnce);
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
