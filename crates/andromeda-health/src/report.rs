//! The `GET /health` response body.
//!
//! A broker extension, not an NGSI-LD resource: it is always this JSON shape,
//! with no `keyValues` or `concise` rendering and no `@context`. It borrows the
//! `id`/`type` framing so the response reads like the rest of the API, and
//! nothing more.

use crate::{
    component::{ComponentDetail, ComponentName},
    status::HealthStatus,
};
use andromeda_ngsi_ld_lexicon::{constant_string, uri::Uri};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

constant_string!(HealthCheckType, "HealthCheck");

/// Top-level health check response for `GET /ngsi-ld/v1/health`.
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

/// Aggregated health status with per-component breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        report::{HealthCheckDetails, HealthCheckResponse, HealthCheckType},
        status::HealthStatus,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use urn_rs::UrnBuilder;

    fn sample_response() -> HealthCheckResponse {
        let id_urn = UrnBuilder::new("ngsi-ld", "HealthCheck:550e8400-e29b-41d4-a716-446655440000")
            .build()
            .expect("a well-formed URN");

        let mut pg_details = BTreeMap::new();
        pg_details.insert("poolSize".to_owned(), Value::from(5));
        pg_details.insert("idleConnections".to_owned(), Value::from(3));
        pg_details.insert("pingLatencyMs".to_owned(), Value::from(2));

        let mut mqtt_details = BTreeMap::new();
        mqtt_details.insert("activeConnections".to_owned(), Value::from(1));
        mqtt_details.insert("unhealthyConnections".to_owned(), Value::from(0));

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
            id: Uri::try_from(id_urn).expect("a minted identifier is within the bound"),
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
