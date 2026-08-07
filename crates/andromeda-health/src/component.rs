//! The subsystems the broker reports on, and what each one reports.

use crate::status::HealthStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Free-form key-value details (pool size, latency, etc.).
    ///
    /// Genuinely free-form: each indicator decides what is worth reporting
    /// about itself, and nothing above reads an individual key. The values are
    /// [`serde_json::Value`] so a count serialises as a number rather than as
    /// a quoted number.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use crate::{
        component::{ComponentDetail, ComponentName},
        status::HealthStatus,
    };
    use serde_json::Value;
    use std::collections::BTreeMap;

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
        let mut details = BTreeMap::new();
        details.insert("poolSize".to_owned(), Value::from(5));

        let component = ComponentDetail {
            status: HealthStatus::Up,
            details,
        };
        let json = serde_json::to_value(&component).expect("serialises");
        pretty_assertions::assert_eq!(json["details"]["poolSize"], Value::from(5));
    }

    #[test]
    fn a_component_with_nothing_to_report_omits_the_details_member() {
        let component = ComponentDetail {
            status: HealthStatus::Up,
            details: BTreeMap::new(),
        };
        let json = serde_json::to_value(&component).expect("serialises");
        assert!(json.get("details").is_none());
    }
}
