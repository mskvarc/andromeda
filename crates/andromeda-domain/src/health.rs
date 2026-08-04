//! Health check domain types for the `/health` endpoint.
//!
//! Returns a custom JSON shape (not an NGSI-LD entity) with component-level
//! health information including database pool statistics and ping latency.

use crate::uri::Uri;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level health check response for `GET /ngsi-ld/v1/health`.
///
/// This endpoint is a broker extension and always returns JSON in this fixed
/// shape (no JSON-LD expansion/compaction variants).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResponse {
    /// Unique identifier of this health check response.
    pub id: Uri,
    /// Response type discriminator (`HealthCheck`).
    #[serde(rename = "type")]
    pub type_: HealthCheckType,
    /// Aggregated health status and per-component details.
    pub health_check: HealthCheckDetails,
}

/// Type discriminator for [`HealthCheckResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, strum::Display, strum::EnumString)]
pub enum HealthCheckType {
    /// `HealthCheck`.
    #[serde(rename = "HealthCheck")]
    #[strum(serialize = "HealthCheck")]
    HealthCheck,
}

/// Aggregated health status with per-component breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthCheckDetails {
    /// Overall health status (worst across all components).
    pub status: HealthStatus,
    /// Per-component health information, keyed by component name.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<ComponentName, ComponentDetail>,
}

/// Health status of an individual component or the overall system.
///
/// `Down` sorts greater than `Up` so that `Iterator::max` yields the worst
/// status across all components.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthStatus {
    /// Component is healthy and operational.
    #[serde(rename = "up")]
    Up,
    /// Component is unhealthy or unreachable.
    #[serde(rename = "down")]
    Down,
}

/// Names of health-checkable system components.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, strum::Display, strum::EnumString)]
pub enum ComponentName {
    /// MQTT notification transport.
    #[serde(rename = "mqtt")]
    #[strum(serialize = "mqtt")]
    Mqtt,
    /// PostgreSQL database backend.
    #[serde(rename = "postgresql")]
    #[strum(serialize = "postgresql")]
    PostgreSql,
}

/// Health details for a single component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentDetail {
    /// Health status of this component.
    pub status: HealthStatus,
    /// Free-form key-value details (pool size, latency, etc.).
    ///
    /// Values are [`serde_json::Value`] so that numeric values serialize as
    /// numbers rather than stringified.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use urn_rs::UrnBuilder;

    fn sample_response() -> HealthCheckResponse {
        let id_urn = UrnBuilder::new("ngsi-ld", "HealthCheck:550e8400-e29b-41d4-a716-446655440000").build().unwrap();

        let mut pg_details = BTreeMap::new();
        pg_details.insert("poolSize".to_owned(), serde_json::Value::from(5));
        pg_details.insert("idleConnections".to_owned(), serde_json::Value::from(3));
        pg_details.insert("pingLatencyMs".to_owned(), serde_json::Value::from(2));

        let mut mqtt_details = BTreeMap::new();
        mqtt_details.insert("activeConnections".to_owned(), serde_json::Value::from(1));
        mqtt_details.insert("unhealthyConnections".to_owned(), serde_json::Value::from(0));

        let mut components = BTreeMap::new();
        components.insert(
            ComponentName::PostgreSql,
            ComponentDetail {
                status: HealthStatus::Up,
                details: pg_details,
            },
        );
        components.insert(
            ComponentName::Mqtt,
            ComponentDetail {
                status: HealthStatus::Up,
                details: mqtt_details,
            },
        );

        HealthCheckResponse {
            id: Uri::Urn(id_urn),
            type_: HealthCheckType::HealthCheck,
            health_check: HealthCheckDetails {
                status: HealthStatus::Up,
                components,
            },
        }
    }

    #[test]
    fn health_check_serializes_to_expected_json() {
        let response = sample_response();
        let json = serde_json::to_value(&response).unwrap();

        let expected = serde_json::json!({
            "id": "urn:ngsi-ld:HealthCheck:550e8400-e29b-41d4-a716-446655440000",
            "type": "HealthCheck",
            "healthCheck": {
                "status": "up",
                "components": {
                    "mqtt": {
                        "status": "up",
                        "details": {
                            "activeConnections": 1,
                            "unhealthyConnections": 0
                        }
                    },
                    "postgresql": {
                        "status": "up",
                        "details": {
                            "poolSize": 5,
                            "idleConnections": 3,
                            "pingLatencyMs": 2
                        }
                    }
                }
            }
        });

        pretty_assertions::assert_eq!(json, expected);
    }

    #[test]
    fn health_check_round_trip_serde() {
        let response = sample_response();
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: HealthCheckResponse = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(deserialized, response);
    }

    #[test]
    fn health_status_down_greater_than_up() {
        assert!(HealthStatus::Down > HealthStatus::Up);
    }

    #[test]
    fn empty_components_omitted_from_json() {
        let details = HealthCheckDetails {
            status: HealthStatus::Up,
            components: BTreeMap::new(),
        };
        let json = serde_json::to_value(&details).unwrap();
        assert!(!json.as_object().unwrap().contains_key("components"));
    }

    #[test]
    fn component_name_serializes_as_postgresql() {
        let json = serde_json::to_value(ComponentName::PostgreSql).unwrap();
        pretty_assertions::assert_eq!(json, serde_json::Value::String("postgresql".to_owned()));
    }

    #[test]
    fn component_name_mqtt_serializes_correctly() {
        let json = serde_json::to_value(ComponentName::Mqtt).unwrap();
        pretty_assertions::assert_eq!(json, serde_json::Value::String("mqtt".to_owned()));
    }

    #[test]
    fn mqtt_sorts_before_postgresql() {
        assert!(ComponentName::Mqtt < ComponentName::PostgreSql);
    }

    #[test]
    fn health_status_serializes_lowercase() {
        let up = serde_json::to_value(HealthStatus::Up).unwrap();
        let down = serde_json::to_value(HealthStatus::Down).unwrap();
        pretty_assertions::assert_eq!(up, serde_json::Value::String("up".to_owned()));
        pretty_assertions::assert_eq!(down, serde_json::Value::String("down".to_owned()));
    }

    #[test]
    fn numeric_detail_value_serializes_as_number() {
        let mut details = BTreeMap::new();
        details.insert("poolSize".to_owned(), serde_json::Value::from(5));

        let component = ComponentDetail {
            status: HealthStatus::Up,
            details,
        };
        let json = serde_json::to_value(&component).unwrap();
        let pool_size = &json["details"]["poolSize"];
        assert!(pool_size.is_number());
        pretty_assertions::assert_eq!(pool_size.as_i64().unwrap(), 5);
    }
}
