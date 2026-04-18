//! MQTT health indicator.
//!
//! Checks the health of all pooled MQTT connections and reports aggregate
//! status alongside per-connection details including `$SYS` broker metadata.

use crate::{
    domain::health::{ComponentDetail, ComponentName, HealthStatus},
    health::HealthIndicator,
    transport::mqtt::MqttTransportPool,
};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

/// Health indicator for MQTT notification transport connections.
///
/// Reports per-connection health state and broker metadata collected from
/// `$SYS` topics (uptime, version, connected clients).
pub struct MqttHealthIndicator {
    /// Shared MQTT connection pool.
    pool: Arc<MqttTransportPool>,
}

impl MqttHealthIndicator {
    /// Create a new MQTT health indicator for the given pool.
    pub const fn new(pool: Arc<MqttTransportPool>) -> MqttHealthIndicator {
        MqttHealthIndicator { pool }
    }
}

impl HealthIndicator for MqttHealthIndicator {
    fn name(&self) -> ComponentName {
        ComponentName::Mqtt
    }

    fn check(&self) -> Pin<Box<dyn Future<Output = ComponentDetail> + Send + '_>> {
        Box::pin(async {
            let summaries = self.pool.connection_health_summaries().await;
            let total = summaries.len();
            let unhealthy_count = summaries.iter().filter(|s| s.health_state != 0).count();

            let status = if self.pool.all_healthy() { HealthStatus::Up } else { HealthStatus::Down };

            let mut details = BTreeMap::new();
            details.insert("activeConnections".to_owned(), serde_json::Value::from(total));
            details.insert("unhealthyConnections".to_owned(), serde_json::Value::from(unhealthy_count));

            if !summaries.is_empty() {
                let connections: Vec<serde_json::Value> = summaries
                    .into_iter()
                    .map(|s| {
                        let health_label = match s.health_state {
                            0 => "healthy",
                            1 => "degraded",
                            2 => "unhealthy",
                            _ => "unknown",
                        };

                        let mut conn = serde_json::Map::new();
                        conn.insert("host".to_owned(), serde_json::Value::String(s.host));
                        conn.insert("port".to_owned(), serde_json::Value::from(s.port));
                        conn.insert("version".to_owned(), serde_json::Value::String(s.version.to_string()));
                        conn.insert("health".to_owned(), serde_json::Value::String(health_label.to_owned()));
                        conn.insert("secondsSinceHealthy".to_owned(), serde_json::Value::from(s.seconds_since_healthy));

                        if let Some(uptime) = s.broker_uptime {
                            conn.insert("brokerUptime".to_owned(), serde_json::Value::String(uptime));
                        }
                        if let Some(version) = s.broker_version {
                            conn.insert("brokerVersion".to_owned(), serde_json::Value::String(version));
                        }
                        if let Some(clients) = s.clients_connected {
                            conn.insert("brokerClientsConnected".to_owned(), serde_json::Value::String(clients));
                        }

                        serde_json::Value::Object(conn)
                    })
                    .collect();
                details.insert("connections".to_owned(), serde_json::Value::Array(connections));
            }

            ComponentDetail { status, details }
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::transport::mqtt::MqttPoolConfig;

    #[test]
    fn mqtt_indicator_name_is_mqtt() {
        let pool = Arc::new(MqttTransportPool::new(MqttPoolConfig::default()));
        let indicator = MqttHealthIndicator::new(pool);
        pretty_assertions::assert_eq!(indicator.name(), ComponentName::Mqtt);
    }

    #[tokio::test]
    async fn empty_pool_reports_up() {
        let pool = Arc::new(MqttTransportPool::new(MqttPoolConfig::default()));
        let indicator = MqttHealthIndicator::new(pool);
        let detail = indicator.check().await;
        pretty_assertions::assert_eq!(detail.status, HealthStatus::Up);
        pretty_assertions::assert_eq!(detail.details["activeConnections"], serde_json::Value::from(0));
        pretty_assertions::assert_eq!(detail.details["unhealthyConnections"], serde_json::Value::from(0));
        assert!(!detail.details.contains_key("connections"));
    }
}
