//! The `HealthCheck` that `GET /ngsi-ld/v1/health` answers: an Andromeda extension no NGSI-LD model covers, read into
//! a type so a test states members, not paths.

use serde::Deserialize;
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use url::Url;

/// The `type` a health document carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum HealthType {
    /// A `HealthCheck`.
    HealthCheck,
}

/// The aggregate state of the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggregateStatus {
    /// Every active component is up.
    Up,
    /// The broker serves with a component impaired.
    Degraded,
}

/// A component with a health indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndicatorName {
    /// The MQTT connections of the notification endpoints.
    Mqtt,
    /// The database pool.
    Postgresql,
}

/// The numbers the database indicator reports; no other member is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostgresqlDetails {
    /// `poolSize`.
    pub pool_size: Number,
    /// `idleConnections`.
    pub idle_connections: Number,
    /// `pingLatencyMs`.
    pub ping_latency_ms: Number,
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
    /// The details read as the database indicator's.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when a member is missing, is not a number or is not one of the three.
    pub fn postgresql_details(&self) -> Result<PostgresqlDetails, serde_json::Error> {
        PostgresqlDetails::deserialize(Value::Object(self.details.clone()))
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
    use crate::health_check::{AggregateStatus, HealthDocument, IndicatorName};
    use googletest::prelude::*;

    const DOCUMENT: &str = r#"{"id":"urn:ngsi-ld:HealthCheck:1","type":"HealthCheck","healthCheck":{"status":"up","components":{"mqtt":{"status":"up","details":{"activeConnections":0}},"postgresql":{"status":"up","details":{"idleConnections":6,"pingLatencyMs":0,"poolSize":7}}}}}"#;

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
        Ok(())
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
