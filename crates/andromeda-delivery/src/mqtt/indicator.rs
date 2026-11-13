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
///
/// A quiet or failed connection makes the component `Degraded` and never
/// `Down`, so no MQTT server decides readiness. Each server is the notification
/// endpoint of a Subscription, chosen by whoever creates it and outside the
/// operator's control, and deliveries to a failed server back off and retry on
/// their own, so taking the broker out of service would recover nothing. The
/// failed connection stays listed in the details.
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
        report(self.pool.connection_health_summaries().await)
    }
}

/// What the component reports for the pooled connections `summaries`
/// describes: `Up` while every one is healthy, `Degraded` otherwise, and each
/// connection listed whatever its health.
fn report(summaries: Vec<MqttConnectionHealthSummary>) -> ComponentDetail {
    let total = summaries.len();
    let unhealthy = summaries.iter().filter(|summary| summary.health == ConnectionHealth::Unhealthy).count();
    let degraded = summaries.iter().filter(|summary| summary.health == ConnectionHealth::Degraded).count();
    let status = match unhealthy + degraded {
        0 => HealthStatus::Up,
        _impaired => HealthStatus::Degraded,
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
            indicator::{MqttHealthIndicator, describe, report},
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

    /// A pooled MQTT 5 connection to port `port` of `broker`, in `health`.
    fn pooled(port: u16, health: ConnectionHealth) -> MqttConnectionHealthSummary {
        MqttConnectionHealthSummary {
            authority: HostPort::from_str(&format!("broker:{port}")).expect("a valid authority"),
            version: MqttVersion::V5,
            health,
            seconds_since_healthy: 90,
            broker_uptime: None,
            broker_version: None,
            clients_connected: None,
        }
    }

    /// The connection [`pooled`] builds, as the health response lists it.
    fn listed(port: u16, health: MqttConnectionState) -> MqttConnectionDetail {
        MqttConnectionDetail {
            broker_clients_connected: None,
            broker_uptime: None,
            broker_version: None,
            health,
            host: "broker".to_owned(),
            port,
            seconds_since_healthy: 90,
            version: MqttProtocolVersion::V5,
        }
    }

    /// The component in `status`, holding `connections` of which `degraded` are quiet and `unhealthy` have failed.
    fn component(status: HealthStatus, connections: Vec<MqttConnectionDetail>, degraded: usize, unhealthy: usize) -> ComponentDetail {
        ComponentDetail {
            status,
            details: Some(ComponentDetails::Mqtt(MqttDetails {
                active_connections: connections.len(),
                connections,
                degraded_connections: degraded,
                unhealthy_connections: unhealthy,
            })),
        }
    }

    #[test]
    fn healthy_connections_report_up_and_are_each_listed() {
        pretty_assertions::assert_eq!(
            report(vec![pooled(1883, ConnectionHealth::Healthy), pooled(1884, ConnectionHealth::Healthy)]),
            component(
                HealthStatus::Up,
                vec![listed(1883, MqttConnectionState::Healthy), listed(1884, MqttConnectionState::Healthy)],
                0,
                0
            )
        );
    }

    #[test]
    fn a_quiet_connection_reports_the_component_degraded_and_is_listed_as_degraded() {
        pretty_assertions::assert_eq!(
            report(vec![pooled(1883, ConnectionHealth::Healthy), pooled(1884, ConnectionHealth::Degraded)]),
            component(
                HealthStatus::Degraded,
                vec![listed(1883, MqttConnectionState::Healthy), listed(1884, MqttConnectionState::Degraded)],
                1,
                0
            )
        );
    }

    #[test]
    fn a_failed_connection_reports_the_component_degraded_rather_than_down_and_is_listed_as_unhealthy() {
        pretty_assertions::assert_eq!(
            report(vec![pooled(1883, ConnectionHealth::Healthy), pooled(1884, ConnectionHealth::Unhealthy)]),
            component(
                HealthStatus::Degraded,
                vec![listed(1883, MqttConnectionState::Healthy), listed(1884, MqttConnectionState::Unhealthy)],
                0,
                1
            )
        );
    }

    #[test]
    fn a_pool_whose_every_connection_failed_still_reports_the_component_degraded() {
        pretty_assertions::assert_eq!(
            report(vec![pooled(1883, ConnectionHealth::Unhealthy), pooled(1884, ConnectionHealth::Unhealthy)]),
            component(
                HealthStatus::Degraded,
                vec![listed(1883, MqttConnectionState::Unhealthy), listed(1884, MqttConnectionState::Unhealthy)],
                0,
                2
            )
        );
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
