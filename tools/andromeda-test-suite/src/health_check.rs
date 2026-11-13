//! The `HealthCheck` that `GET /readyz` on the management listener answers: an Andromeda extension no NGSI-LD model covers, read into
//! a type so a test states members, not paths.

use serde::Deserialize;
use serde_json::{Map, Number, Value};
use std::{collections::BTreeMap, net::Ipv6Addr};
use url::{Host, Url};

/// The `type` a health document carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum HealthType {
    /// A `HealthCheck`.
    HealthCheck,
}

/// The aggregate state of the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AggregateStatus {
    /// Every active component is up.
    Up,
    /// The broker serves with a component impaired.
    Degraded,
    /// A component is down, and the probe answers 503.
    Down,
}

/// A component with a health indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IndicatorName {
    /// The background tasks of the broker and of its loaded tenants.
    BackgroundTasks,
    /// The MQTT connections of the notification endpoints.
    Mqtt,
    /// The database pool.
    Postgresql,
}

/// The numbers the database indicator reports once the database answered; no other member is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostgresqlDetails {
    /// `poolSize`: the connections open in the platform pool.
    pub pool_size: Number,
    /// `idleConnections`: the idle connections of the platform pool.
    pub idle_connections: Number,
    /// `maxConnections`: the most connections the platform pool opens.
    pub max_connections: Number,
    /// `pingLatencyMs`.
    pub ping_latency_ms: Number,
}

impl PostgresqlDetails {
    /// Whether every connection the platform pool may open is open and in use.
    #[must_use]
    pub fn platform_pool_saturated(&self) -> bool {
        self.pool_size == self.max_connections && self.idle_connections.as_u64() == Some(0)
    }
}

/// What the database indicator reports once the database did not answer; no other member is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostgresqlFailure {
    /// `error`: why the ping failed.
    pub error: String,
}

/// A background task that stopped for good, as the background tasks indicator lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoppedTask {
    /// `name`: the task's registered name.
    pub name: String,
    /// `tenant`: the tenant's name, `@default` for the unnamed default tenant, absent for a task of the whole broker.
    pub tenant: Option<String>,
    /// `criticality`: `observed` or `tenantDegrading`.
    pub criticality: String,
    /// `failureClass`: `transient`, `permanent` or `invariantViolation`.
    pub failure_class: String,
    /// `failureSummary`.
    pub failure_summary: String,
}

/// What the background tasks indicator reports; no other member is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundTaskDetails {
    /// `stoppedTasks`: empty while every task runs.
    pub stopped_tasks: Vec<StoppedTask>,
}

/// The state of a connection to an MQTT server, as the MQTT indicator lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MqttConnectionHealth {
    /// `healthy`: the server answers.
    Healthy,
    /// `degraded`: nothing has come from the server for longer than its keep-alive allows.
    Degraded,
    /// `unhealthy`: the connection has failed.
    Unhealthy,
}

/// The MQTT protocol version of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum MqttProtocol {
    /// `v3.1.1`.
    #[serde(rename = "v3.1.1")]
    V311,
    /// `v5`.
    #[serde(rename = "v5")]
    V5,
}

/// A connection to an MQTT server, as the MQTT indicator lists it; no other member is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MqttConnection {
    /// `host`: the server's host as the endpoint names it, an IPv6 address without brackets.
    pub host: String,
    /// `port`.
    pub port: u16,
    /// `health`.
    pub health: MqttConnectionHealth,
    /// `version`.
    pub version: MqttProtocol,
    /// `secondsSinceHealthy`.
    pub seconds_since_healthy: u64,
    /// `brokerUptime`: what the server published on `$SYS/broker/uptime`, absent until it publishes one.
    pub broker_uptime: Option<String>,
    /// `brokerVersion`: what the server published on `$SYS/broker/version`, absent until it publishes one.
    pub broker_version: Option<String>,
    /// `brokerClientsConnected`: what the server published on `$SYS/broker/clients/connected`, absent until it
    /// publishes one.
    pub broker_clients_connected: Option<String>,
}

impl MqttConnection {
    /// `host` as a URL host, so it compares with the host of an endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`url::ParseError`] when `host` is neither an IP address nor a domain.
    pub fn url_host(&self) -> Result<Host<String>, url::ParseError> {
        match self.host.parse::<Ipv6Addr>() {
            Ok(address) => Ok(Host::Ipv6(address)),
            Err(_not_ipv6) => Host::parse(&self.host),
        }
    }
}

/// What the MQTT indicator reports; no other member is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MqttDetails {
    /// `activeConnections`: the connections the default tenant holds.
    pub active_connections: u64,
    /// `connections`: each of them, whatever its health; absent while there is none.
    #[serde(default)]
    pub connections: Vec<MqttConnection>,
    /// `degradedConnections`.
    pub degraded_connections: u64,
    /// `unhealthyConnections`.
    pub unhealthy_connections: u64,
}

/// One component's indicator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Indicator {
    /// `status`: any string; the database indicator is `up` on a serving broker.
    pub status: String,
    /// `details`: a JSON object, null never.
    pub details: Map<String, Value>,
}

impl Indicator {
    /// The details read as the background tasks indicator's.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `stoppedTasks` is missing or a task lacks a member, or when another member is present.
    pub fn background_task_details(&self) -> Result<BackgroundTaskDetails, serde_json::Error> {
        BackgroundTaskDetails::deserialize(Value::Object(self.details.clone()))
    }

    /// The details read as the MQTT indicator's.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when a count is missing or a connection lacks a member, or when another member is
    /// present.
    pub fn mqtt_details(&self) -> Result<MqttDetails, serde_json::Error> {
        MqttDetails::deserialize(Value::Object(self.details.clone()))
    }

    /// The details read as the database indicator's.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when a member is missing, is not a number or is not one of the four.
    pub fn postgresql_details(&self) -> Result<PostgresqlDetails, serde_json::Error> {
        PostgresqlDetails::deserialize(Value::Object(self.details.clone()))
    }

    /// The details read as the database indicator's once the database did not answer.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `error` is missing or another member is present.
    pub fn postgresql_failure(&self) -> Result<PostgresqlFailure, serde_json::Error> {
        PostgresqlFailure::deserialize(Value::Object(self.details.clone()))
    }
}

/// The `healthCheck` member: the aggregate and the indicators of the active components.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HealthReport {
    /// `status`.
    pub status: AggregateStatus,
    /// `components`: only the indicators of the active components are reported.
    pub components: BTreeMap<IndicatorName, Indicator>,
}

/// The document the health endpoint answers: `id`, `type` and `healthCheck`, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthDocument {
    /// `id`: `urn:ngsi-ld:HealthCheck:` and a token.
    pub id: Url,
    /// `type`.
    #[serde(rename = "type")]
    pub health_type: HealthType,
    /// `healthCheck`.
    pub health_check: HealthReport,
}

#[cfg(test)]
mod tests {
    use crate::health_check::{AggregateStatus, HealthDocument, IndicatorName, MqttConnection, MqttConnectionHealth, MqttProtocol};
    use googletest::prelude::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use url::Host;

    const DOCUMENT: &str = r#"{"id":"urn:ngsi-ld:HealthCheck:1","type":"HealthCheck","healthCheck":{"status":"up","components":{"backgroundTasks":{"status":"up","details":{"stoppedTasks":[]}},"mqtt":{"status":"up","details":{"activeConnections":0}},"postgresql":{"status":"up","details":{"idleConnections":6,"maxConnections":32,"pingLatencyMs":0,"poolSize":7}}}}}"#;

    #[gtest]
    fn a_document_reads_its_aggregate_and_its_indicators() -> Result<()> {
        let document: HealthDocument = serde_json::from_str(DOCUMENT)?;
        expect_that!(document.health_check.status, eq(AggregateStatus::Up));
        let database = document.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
        expect_that!(database.postgresql_details(), ok(anything()));
        expect_that!(
            document.health_check.components.get(&IndicatorName::Mqtt).or_fail()?.postgresql_details(),
            err(anything())
        );
        let tasks = document.health_check.components.get(&IndicatorName::BackgroundTasks).or_fail()?;
        expect_that!(tasks.background_task_details().map(|details| details.stopped_tasks), ok(is_empty()));
        Ok(())
    }

    #[gtest]
    fn the_mqtt_details_list_each_connection_with_its_health_and_its_host() -> Result<()> {
        let text = DOCUMENT.replacen(
            r#""mqtt":{"status":"up","details":{"activeConnections":0}}"#,
            concat!(
                r#""mqtt":{"status":"degraded","details":{"activeConnections":2,"connections":["#,
                r#"{"health":"unhealthy","host":"172.18.0.9","port":40123,"secondsSinceHealthy":4,"version":"v5"},"#,
                r#"{"brokerUptime":"120 seconds","health":"healthy","host":"fd00::9","port":1883,"secondsSinceHealthy":0,"version":"v3.1.1"}"#,
                r#"],"degradedConnections":0,"unhealthyConnections":1}}"#
            ),
            1,
        );
        let document: HealthDocument = serde_json::from_str(&text)?;
        let details = document.health_check.components.get(&IndicatorName::Mqtt).or_fail()?.mqtt_details()?;
        expect_that!(details.unhealthy_connections, eq(1));
        expect_that!(
            details
                .connections
                .iter()
                .map(|connection| (connection.health, connection.version))
                .collect::<Vec<_>>(),
            elements_are![
                eq(&(MqttConnectionHealth::Unhealthy, MqttProtocol::V5)),
                eq(&(MqttConnectionHealth::Healthy, MqttProtocol::V311))
            ]
        );
        expect_that!(
            details.connections.iter().map(MqttConnection::url_host).collect::<Vec<_>>(),
            elements_are![
                ok(eq(&Host::<String>::Ipv4(Ipv4Addr::new(172, 18, 0, 9)))),
                ok(eq(&Host::<String>::Ipv6("fd00::9".parse::<Ipv6Addr>()?)))
            ]
        );
        Ok(())
    }

    #[gtest]
    fn mqtt_details_without_a_connection_read_an_empty_list() -> Result<()> {
        let text = DOCUMENT.replacen(
            r#""activeConnections":0}"#,
            r#""activeConnections":0,"degradedConnections":0,"unhealthyConnections":0}"#,
            1,
        );
        let document: HealthDocument = serde_json::from_str(&text)?;
        let details = document.health_check.components.get(&IndicatorName::Mqtt).or_fail()?.mqtt_details()?;
        verify_that!(details.connections, is_empty())
    }

    #[gtest]
    fn a_platform_pool_with_every_connection_open_and_none_idle_reads_saturated() -> Result<()> {
        let idle: HealthDocument = serde_json::from_str(DOCUMENT)?;
        let database = idle.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
        expect_that!(database.postgresql_details()?.platform_pool_saturated(), eq(false));
        let text = DOCUMENT.replacen(
            r#""idleConnections":6,"maxConnections":32,"pingLatencyMs":0,"poolSize":7"#,
            r#""idleConnections":0,"maxConnections":2,"pingLatencyMs":0,"poolSize":2"#,
            1,
        );
        let saturated: HealthDocument = serde_json::from_str(&text)?;
        let database = saturated.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
        verify_that!(database.postgresql_details()?.platform_pool_saturated(), eq(true))
    }

    #[gtest]
    fn a_document_with_the_database_down_reads_why() -> Result<()> {
        let text = DOCUMENT
            .replacen(r#""healthCheck":{"status":"up""#, r#""healthCheck":{"status":"down""#, 1)
            .replacen(
                r#""postgresql":{"status":"up","details":{"idleConnections":6,"maxConnections":32,"pingLatencyMs":0,"poolSize":7}}"#,
                r#""postgresql":{"status":"down","details":{"error":"the database did not answer within 1000 ms"}}"#,
                1,
            );
        let document: HealthDocument = serde_json::from_str(&text)?;
        expect_that!(document.health_check.status, eq(AggregateStatus::Down));
        let database = document.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
        expect_that!(database.postgresql_details(), err(anything()));
        verify_that!(
            database.postgresql_failure().map(|failure| failure.error),
            ok(eq("the database did not answer within 1000 ms"))
        )
    }

    #[gtest]
    fn an_indicator_of_an_unknown_component_is_refused() {
        let text = DOCUMENT.replacen(r#""mqtt""#, r#""kafka""#, 1);
        expect_that!(serde_json::from_str::<HealthDocument>(&text), err(anything()));
    }

    #[gtest]
    fn a_member_beside_the_documented_three_is_refused() {
        let text = DOCUMENT.replacen(r#""type":"HealthCheck""#, r#""type":"HealthCheck","@context":"x""#, 1);
        expect_that!(serde_json::from_str::<HealthDocument>(&text), err(anything()));
    }
}
