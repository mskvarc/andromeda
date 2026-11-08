//! What the MQTT pool reports to `/health`.

use crate::{
    endpoint::mqtt::MqttVersion,
    mqtt::{
        health::{ConnectionHealth, MqttConnectionHealthSummary},
        pool::MqttTransportPool,
    },
};
use andromeda_health::{
    component::{ComponentDetail, ComponentName},
    details::ComponentDetails,
    indicator::HealthIndicator,
    mqtt_details::{MqttConnectionDetail, MqttConnectionState, MqttDetails, MqttProtocolVersion},
    status::HealthStatus,
};
use async_trait::async_trait;
use std::sync::Arc;

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
    #[must_use]
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

        ComponentDetail {
            status,
            details: Some(ComponentDetails::Mqtt(MqttDetails {
                active_connections: total,
                connections: summaries.into_iter().map(describe).collect(),
                degraded_connections: degraded,
                unhealthy_connections: unhealthy,
            })),
        }
    }
}

/// One connection, as it appears in the health response.
fn describe(summary: MqttConnectionHealthSummary) -> MqttConnectionDetail {
    MqttConnectionDetail {
        broker_clients_connected: summary.clients_connected,
        broker_uptime: summary.broker_uptime,
        broker_version: summary.broker_version,
        health: match summary.health {
            ConnectionHealth::Healthy => MqttConnectionState::Healthy,
            ConnectionHealth::Degraded => MqttConnectionState::Degraded,
            ConnectionHealth::Unhealthy => MqttConnectionState::Unhealthy,
        },
        host: summary.authority.host_str().into_owned(),
        port: summary.authority.port(),
        seconds_since_healthy: summary.seconds_since_healthy,
        version: match summary.version {
            MqttVersion::V311 => MqttProtocolVersion::V311,
            MqttVersion::V5 => MqttProtocolVersion::V5,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint::mqtt::MqttVersion,
        host_port::HostPort,
        mqtt::{
            client_tls::mqtt_client_tls,
            config::MqttPoolConfig,
            dial::RumqttcDialer,
            health::{ConnectionHealth, MqttConnectionHealthSummary},
            indicator::{MqttHealthIndicator, describe},
            pool::MqttTransportPool,
        },
    };
    use andromeda_health::{component::ComponentName, details::ComponentDetails, indicator::HealthIndicator, mqtt_details::MqttDetails, status::HealthStatus};
    use serde_json::json;
    use std::{str::FromStr, sync::Arc};

    fn indicator() -> MqttHealthIndicator {
        MqttHealthIndicator::new(Arc::new(MqttTransportPool::new(
            MqttPoolConfig::default(),
            Arc::new(RumqttcDialer::new(
                mqtt_client_tls().expect("the system trust store yields a TLS client configuration"),
            )),
        )))
    }

    #[test]
    fn the_indicator_reports_under_the_mqtt_component_name() {
        pretty_assertions::assert_eq!(indicator().name(), ComponentName::Mqtt);
    }

    #[tokio::test]
    async fn a_broker_that_has_opened_no_connection_reports_up_and_lists_none() {
        let detail = indicator().check().await;
        pretty_assertions::assert_eq!(detail.status, HealthStatus::Up);
        pretty_assertions::assert_eq!(
            detail.details,
            Some(ComponentDetails::Mqtt(MqttDetails {
                active_connections: 0,
                connections: Vec::new(),
                degraded_connections: 0,
                unhealthy_connections: 0,
            }))
        );
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

        let rendered = serde_json::to_value(&rendered).expect("serialises");

        pretty_assertions::assert_eq!(rendered["host"], json!("broker"));
        pretty_assertions::assert_eq!(rendered["port"], json!(1883));
        pretty_assertions::assert_eq!(rendered["health"], json!("degraded"));
        pretty_assertions::assert_eq!(rendered["version"], json!("v5"));
        pretty_assertions::assert_eq!(rendered["secondsSinceHealthy"], json!(90));
        pretty_assertions::assert_eq!(rendered["brokerUptime"], json!("120 seconds"));
        assert!(rendered.get("brokerVersion").is_none());
    }
}
