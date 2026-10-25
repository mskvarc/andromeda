//! The subsystems the broker reports on, and what each one reports.

use crate::{details::ComponentDetails, status::HealthStatus};
use serde::{Deserialize, Serialize};

/// Names of health-checkable system components.
///
/// The name is the JSON key a component's details appear under, so adding a
/// variant changes the response shape. It is deliberately a closed set rather
/// than a free string: a component that cannot name itself here has no
/// business claiming a place in the report.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentName {
    /// MQTT notification transport.
    #[serde(rename = "mqtt")]
    Mqtt,
    /// PostgreSQL database backend.
    #[serde(rename = "postgresql")]
    PostgreSql,
}

/// Health details for a single component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentDetail {
    /// Health status of this component.
    pub status: HealthStatus,
    /// What the component reports about itself (pool size, latency, etc.).
    ///
    /// Absent when the component has nothing to report beyond its status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ComponentDetails>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        component::{ComponentDetail, ComponentName},
        details::ComponentDetails,
        postgresql_details::PostgreSqlDetails,
        status::HealthStatus,
    };
    use serde_json::{Value, json};

    #[test]
    fn a_component_name_serialises_as_the_key_the_response_uses() {
        pretty_assertions::assert_eq!(
            serde_json::to_value(ComponentName::PostgreSql).expect("serialises"),
            Value::String("postgresql".to_owned())
        );
        pretty_assertions::assert_eq!(serde_json::to_value(ComponentName::Mqtt).expect("serialises"), Value::String("mqtt".to_owned()));
    }

    #[test]
    fn component_names_sort_so_the_response_key_order_is_stable() {
        assert!(ComponentName::Mqtt < ComponentName::PostgreSql);
    }

    #[test]
    fn a_numeric_detail_serialises_as_a_number_rather_than_as_a_string() {
        let component = ComponentDetail {
            status: HealthStatus::Up,
            details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Reachable {
                idle_connections: 3,
                ping_latency_ms: 2,
                pool_size: 5,
            })),
        };
        let json = serde_json::to_value(&component).expect("serialises");
        pretty_assertions::assert_eq!(json["details"]["poolSize"], json!(5));
    }

    #[test]
    fn a_component_with_nothing_to_report_omits_the_details_member() {
        let component = ComponentDetail {
            status: HealthStatus::Up,
            details: None,
        };
        let json = serde_json::to_value(&component).expect("serialises");
        assert!(json.get("details").is_none());
    }
}
