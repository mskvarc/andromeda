use crate::{
    domain::{serde_helpers, subscription::SubscriptionStatus},
    query::q::Expr,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use url::Url;

// ---------------------------------------------------------------------------
// Notification types from NGSI-LD spec (ETSI GS CIM 009 clause 5.3.1)
// ---------------------------------------------------------------------------

/// Maps NGSI-LD entity mutation operations to spec trigger reasons.
#[derive(Clone, Debug)]
pub enum TriggerReason {
    EntityCreated,
    EntityUpdated,
    EntityDeleted,
    AttributeCreated,
    AttributeUpdated,
    AttributeDeleted,
}

impl TriggerReason {
    /// Returns the NGSI-LD notificationTrigger string for this reason.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EntityCreated => "entityCreated",
            Self::EntityUpdated => "entityUpdated",
            Self::EntityDeleted => "entityDeleted",
            Self::AttributeCreated => "attributeCreated",
            Self::AttributeUpdated => "attributeUpdated",
            Self::AttributeDeleted => "attributeDeleted",
        }
    }

    /// Default triggers per NGSI-LD spec.
    pub fn defaults() -> Vec<&'static str> {
        vec![
            "entityCreated",
            "entityUpdated",
            "entityDeleted",
            "attributeCreated",
            "attributeUpdated",
            "attributeDeleted",
        ]
    }
}

/// Represents an entity mutation that may trigger notifications.
#[derive(Clone, Debug)]
pub struct ChangeEvent {
    pub entity_id: String,
    pub entity_types: Vec<String>,
    pub trigger_reason: TriggerReason,
    pub changed_attributes: Vec<String>,
    pub occurred_at: DateTime<Utc>,
    /// For EntityDeleted events, a snapshot of the entity before deletion (expanded form).
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation (expanded form), for showChanges support.
    pub previous_attributes: Option<Value>,
    /// For EntityDeleted events, a snapshot of (createdAt, modifiedAt, expiresAt) captured before deletion.
    pub sys_attrs_snapshot: Option<(DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>,
}

impl ChangeEvent {
    pub fn new(entity_id: String, entity_types: Vec<String>, trigger_reason: TriggerReason) -> ChangeEvent {
        ChangeEvent {
            entity_id,
            entity_types,
            trigger_reason,
            changed_attributes: Vec::new(),
            occurred_at: Utc::now(),
            entity_snapshot: None,
            previous_attributes: None,
            sys_attrs_snapshot: None,
        }
    }

    pub fn with_changed_attributes(mut self, attrs: Vec<String>) -> ChangeEvent {
        self.changed_attributes = attrs;
        self
    }

    pub fn with_snapshot(mut self, snapshot: Value) -> ChangeEvent {
        self.entity_snapshot = Some(snapshot);
        self
    }

    pub fn with_previous(mut self, previous: Value) -> ChangeEvent {
        self.previous_attributes = Some(previous);
        self
    }

    pub fn with_optional_previous(mut self, previous: Option<Value>) -> ChangeEvent {
        self.previous_attributes = previous;
        self
    }

    pub fn with_sys_attrs(mut self, sys_attrs: Option<(DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>) -> ChangeEvent {
        self.sys_attrs_snapshot = sys_attrs;
        self
    }
}

/// The NGSI-LD Notification payload structure (clause 5.3.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: String,

    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,

    #[serde(rename = "notifiedAt", with = "serde_helpers::rfc3339")]
    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,
}

/// Trigger reasons for CSR notifications (spec clause 5.3.2).
#[derive(Clone, Debug, PartialEq)]
pub enum CsrTriggerReason {
    NewlyMatching,
    Updated,
    NoLongerMatching,
}

impl CsrTriggerReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NewlyMatching => "newlyMatching",
            Self::Updated => "updated",
            Self::NoLongerMatching => "noLongerMatching",
        }
    }
}

/// Represents a CSR mutation that may trigger CSR subscription notifications.
#[derive(Clone, Debug)]
pub struct CsrChangeEvent {
    pub csr_id: String,
    pub trigger_reason: CsrTriggerReason,
    pub occurred_at: DateTime<Utc>,
    /// For delete events, a snapshot of the CSR before deletion.
    pub csr_snapshot: Option<Value>,
}

/// The NGSI-LD CsourceNotification payload structure (clause 5.3.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsourceNotification {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: String,

    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,

    #[serde(rename = "notifiedAt", with = "serde_helpers::rfc3339")]
    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "triggerReason")]
    pub trigger_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Endpoint transport and subscription compilation (existing types)
// ---------------------------------------------------------------------------

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
