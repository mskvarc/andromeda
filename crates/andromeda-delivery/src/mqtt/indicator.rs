//! What the MQTT pool reports to `/health`.

use crate::mqtt::{
    health::{ConnectionHealth, MqttConnectionHealthSummary},
    pool::MqttTransportPool,
};
use andromeda_health::{
    component::{ComponentDetail, ComponentName},
    indicator::HealthIndicator,
    status::HealthStatus,
};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc};

/// Health indicator for MQTT notification delivery.
///
/// Reports per-connection health and whatever broker metadata arrived over
/// `$SYS`. The probe is a read of state the event loops already maintain, so
/// calling `/health` does not touch a broker.
pub struct MqttHealthIndicator {
    /// The pool being reported on.
    pool: Arc<MqttTransportPool>,
}

impl MqttHealthIndicator {
    /// Report on the given pool.
    pub const fn new(pool: Arc<MqttTransportPool>) -> MqttHealthIndicator {
        MqttHealthIndicator { pool }
    }
}

#[async_trait]
impl HealthIndicator for MqttHealthIndicator {
    fn name(&self) -> ComponentName {
        ComponentName::Mqtt
    }

    async fn check(&self) -> ComponentDetail {
        let summaries = self.pool.connection_health_summaries().await;
        let total = summaries.len();
        let unhealthy = summaries.iter().filter(|summary| summary.health == ConnectionHealth::Unhealthy).count();
        let degraded = summaries.iter().filter(|summary| summary.health == ConnectionHealth::Degraded).count();
        let status = match (unhealthy, degraded) {
            (0, 0) => HealthStatus::Up,
            (0, _) => HealthStatus::Degraded,
            (_, _) => HealthStatus::Down,
        };

        let mut details = BTreeMap::new();
        details.insert("activeConnections".to_owned(), Value::from(total));
        details.insert("unhealthyConnections".to_owned(), Value::from(unhealthy));
        details.insert("degradedConnections".to_owned(), Value::from(degraded));
        if !summaries.is_empty() {
            details.insert("connections".to_owned(), Value::Array(summaries.into_iter().map(describe).collect()));
        }

        ComponentDetail { status, details }
    }
}

/// One connection, as it appears in the health response.
fn describe(summary: MqttConnectionHealthSummary) -> Value {
    let mut connection = Map::new();
    connection.insert("host".to_owned(), Value::String(summary.authority.host_str().into_owned()));
    connection.insert("port".to_owned(), Value::from(summary.authority.port()));
    connection.insert("version".to_owned(), Value::String(summary.version.to_string()));
    connection.insert("health".to_owned(), Value::String(summary.health.to_string()));
    connection.insert("secondsSinceHealthy".to_owned(), Value::from(summary.seconds_since_healthy));

    if let Some(uptime) = summary.broker_uptime {
        connection.insert("brokerUptime".to_owned(), Value::String(uptime));
    }
    if let Some(version) = summary.broker_version {
        connection.insert("brokerVersion".to_owned(), Value::String(version));
    }
    if let Some(clients) = summary.clients_connected {
        connection.insert("brokerClientsConnected".to_owned(), Value::String(clients));
    }

    Value::Object(connection)
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::mqtt::MqttVersion,
        host_port::HostPort,
        mqtt::{
            config::MqttPoolConfig,
            dial::RumqttcDialer,
            health::{ConnectionHealth, MqttConnectionHealthSummary},
            indicator::{MqttHealthIndicator, describe},
            pool::MqttTransportPool,
        },
    };
    use andromeda_health::{component::ComponentName, indicator::HealthIndicator, status::HealthStatus};
    use serde_json::{Value, json};
    use std::{str::FromStr, sync::Arc};

    fn indicator() -> MqttHealthIndicator {
        MqttHealthIndicator::new(Arc::new(MqttTransportPool::new(MqttPoolConfig::default(), Arc::new(RumqttcDialer))))
    }

    #[test]
    fn the_indicator_reports_under_the_mqtt_component_name() {
        pretty_assertions::assert_eq!(indicator().name(), ComponentName::Mqtt);
    }

    #[tokio::test]
    async fn a_broker_that_has_opened_no_connection_reports_up_and_lists_none() {
        let detail = indicator().check().await;
        pretty_assertions::assert_eq!(detail.status, HealthStatus::Up);
        pretty_assertions::assert_eq!(detail.details["activeConnections"], Value::from(0));
        pretty_assertions::assert_eq!(detail.details["unhealthyConnections"], Value::from(0));
        pretty_assertions::assert_eq!(detail.details["degradedConnections"], Value::from(0));
        assert!(!detail.details.contains_key("connections"));
    }

    #[test]
    fn a_degraded_connection_reports_its_health_by_name_rather_than_as_a_number() {
        let rendered = describe(MqttConnectionHealthSummary {
            authority: HostPort::from_str("broker:1883").expect("a valid authority"),
            version: MqttVersion::V5,
            health: ConnectionHealth::Degraded,
            seconds_since_healthy: 90,
            broker_uptime: Some("120 seconds".to_owned()),
            broker_version: None,
            clients_connected: None,
        });

        pretty_assertions::assert_eq!(rendered["host"], json!("broker"));
        pretty_assertions::assert_eq!(rendered["port"], json!(1883));
        pretty_assertions::assert_eq!(rendered["health"], json!("degraded"));
        pretty_assertions::assert_eq!(rendered["version"], json!("v5"));
        pretty_assertions::assert_eq!(rendered["secondsSinceHealthy"], json!(90));
        pretty_assertions::assert_eq!(rendered["brokerUptime"], json!("120 seconds"));
        assert!(rendered.get("brokerVersion").is_none());
    }
}
