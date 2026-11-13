//! The `GET /readyz` response body.
//!
//! A broker extension, not an NGSI-LD resource: it is always this JSON shape,
//! with no `keyValues` or `concise` rendering and no `@context`. It borrows the
//! `id`/`type` framing so the response reads like the rest of the API, and
//! nothing more.

use crate::{
    component::{ComponentDetail, ComponentName},
    status::HealthStatus,
};
use andromeda_ngsi_ld_lexicon::{constant_string, identifier::Identifier};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

constant_string!(pub HealthCheckType, "HealthCheck");

/// Top-level health check response for `GET /readyz` on the management plane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResponse {
    /// Unique identifier of this health check response.
    pub id: Identifier,
    /// Response type discriminator (`HealthCheck`).
    #[serde(rename = "type")]
    pub type_: HealthCheckType,
    /// Aggregated health status and per-component details.
    pub health_check: HealthCheckDetails,
}

/// Aggregated health status with per-component breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckDetails {
    /// Overall health status (worst across all components).
    pub status: HealthStatus,
    /// Per-component health information, keyed by component name.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<ComponentName, ComponentDetail>,
}

#[cfg(test)]
mod tests {
    use crate::{
        component::{ComponentDetail, ComponentName},
        details::ComponentDetails,
        mqtt_details::MqttDetails,
        postgresql_details::PostgreSqlDetails,
        report::{HealthCheckDetails, HealthCheckResponse, HealthCheckType},
        status::HealthStatus,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::json;
    use std::collections::BTreeMap;
    use urn_rs::{Urn, urn};

    fn sample_response() -> HealthCheckResponse {
        let id_urn = Urn::from(urn!("urn:ngsi-ld:HealthCheck:550e8400-e29b-41d4-a716-446655440000"));

        let mut components = BTreeMap::new();
        components.insert(
            ComponentName::PostgreSql,
            ComponentDetail {
                status: HealthStatus::Up,
                details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Reachable {
                    idle_connections: 3,
                    max_connections: 8,
                    ping_latency_ms: 2,
                    pool_size: 5,
                })),
            },
        );
        components.insert(
            ComponentName::Mqtt,
            ComponentDetail {
                status: HealthStatus::Up,
                details: Some(ComponentDetails::Mqtt(MqttDetails {
                    active_connections: 1,
                    connections: Vec::new(),
                    degraded_connections: 0,
                    unhealthy_connections: 0,
                })),
            },
        );

        HealthCheckResponse {
            id: Identifier::try_from(id_urn).expect("a minted identifier is within the bound"),
            type_: HealthCheckType,
            health_check: HealthCheckDetails {
                status: HealthStatus::Up,
                components,
            },
        }
    }

    #[test]
    fn the_response_serialises_to_the_documented_shape() {
        let json = serde_json::to_value(sample_response()).expect("serialises");

        let expected = json!({
            "id": "urn:ngsi-ld:HealthCheck:550e8400-e29b-41d4-a716-446655440000",
            "type": "HealthCheck",
            "healthCheck": {
                "status": "up",
                "components": {
                    "mqtt": {
                        "status": "up",
                        "details": {
                            "activeConnections": 1,
                            "degradedConnections": 0,
                            "unhealthyConnections": 0
                        }
                    },
                    "postgresql": {
                        "status": "up",
                        "details": {
                            "poolSize": 5,
                            "idleConnections": 3,
                            "maxConnections": 8,
                            "pingLatencyMs": 2
                        }
                    }
                }
            }
        });

        pretty_assertions::assert_eq!(json, expected);
    }

    #[test]
    fn the_response_round_trips_through_serde() {
        let response = sample_response();
        let json = serde_json::to_string(&response).expect("serialises");
        let deserialized: HealthCheckResponse = serde_json::from_str(&json).expect("parses");
        pretty_assertions::assert_eq!(deserialized, response);
    }

    #[test]
    fn a_broker_with_no_registered_indicators_omits_the_components_member() {
        let details = HealthCheckDetails {
            status: HealthStatus::Up,
            components: BTreeMap::new(),
        };
        let json = serde_json::to_value(&details).expect("serialises");
        assert!(json.get("components").is_none());
    }
}
