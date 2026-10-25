//! What the MQTT transport reports about itself.

use serde::{Deserialize, Serialize};

/// Health of an MQTT connection, as the response names it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MqttConnectionState {
    /// The broker is answering.
    Healthy,
    /// Nothing has been heard for longer than expected.
    Degraded,
    /// The connection has failed.
    Unhealthy,
}

/// MQTT protocol version of a connection, as the response names it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MqttProtocolVersion {
    /// MQTT 3.1.1.
    #[serde(rename = "v3.1.1")]
    V311,
    /// MQTT 5.
    #[serde(rename = "v5")]
    V5,
}

/// One connection, as it appears in the health response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MqttConnectionDetail {
    /// Connected client count the broker published on `$SYS`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker_clients_connected: Option<String>,
    /// Broker uptime the broker published on `$SYS`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker_uptime: Option<String>,
    /// Broker version the broker published on `$SYS`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker_version: Option<String>,
    /// Health of this connection.
    pub health: MqttConnectionState,
    /// Host of the broker.
    pub host: String,
    /// Port of the broker.
    pub port: u16,
    /// Seconds since this connection was last known healthy.
    pub seconds_since_healthy: u64,
    /// MQTT protocol version of this connection.
    pub version: MqttProtocolVersion,
}

/// What the MQTT transport reports: connection counts and each connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MqttDetails {
    /// Connections the pool holds.
    pub active_connections: usize,
    /// Each connection; absent while the pool holds none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<MqttConnectionDetail>,
    /// Connections that are degraded.
    pub degraded_connections: usize,
    /// Connections that have failed.
    pub unhealthy_connections: usize,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::mqtt_details::{MqttConnectionDetail, MqttConnectionState, MqttDetails, MqttProtocolVersion};
    use serde_json::json;

    #[test]
    fn a_pool_with_no_connections_omits_the_connections_member() {
        let details = MqttDetails {
            active_connections: 0,
            connections: Vec::new(),
            degraded_connections: 0,
            unhealthy_connections: 0,
        };
        pretty_assertions::assert_eq!(
            serde_json::to_value(&details).expect("serialises"),
            json!({"activeConnections": 0, "degradedConnections": 0, "unhealthyConnections": 0})
        );
    }

    #[test]
    fn a_connection_names_its_health_and_version_and_omits_unpublished_broker_metadata() {
        let connection = MqttConnectionDetail {
            broker_clients_connected: None,
            broker_uptime: Some("120 seconds".to_owned()),
            broker_version: None,
            health: MqttConnectionState::Degraded,
            host: "broker".to_owned(),
            port: 1883,
            seconds_since_healthy: 90,
            version: MqttProtocolVersion::V311,
        };
        pretty_assertions::assert_eq!(
            serde_json::to_value(&connection).expect("serialises"),
            json!({
                "brokerUptime": "120 seconds",
                "health": "degraded",
                "host": "broker",
                "port": 1883,
                "secondsSinceHealthy": 90,
                "version": "v3.1.1"
            })
        );
    }
}
