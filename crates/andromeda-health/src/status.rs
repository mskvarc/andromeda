//! Whether a component, or the broker as a whole, is answering.

use serde::{Deserialize, Serialize};

/// Health status of an individual component or the overall system.
///
/// `Down` sorts greater than `Degraded`, which sorts greater than `Up`, so that
/// taking the maximum across components yields the worst one. That ordering is load-bearing: it is what makes
/// [`crate::aggregate::Health::check_all`] a fold rather than a chain of
/// conditionals, and it is why a third status would have to be inserted at the
/// right rank rather than appended.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthStatus {
    /// Component is healthy and operational.
    #[serde(rename = "up")]
    Up,
    /// Component is answering but has reduced capability.
    #[serde(rename = "degraded")]
    Degraded,
    /// Component is unhealthy or unreachable.
    #[serde(rename = "down")]
    Down,
}

#[cfg(test)]
mod tests {
    use crate::status::HealthStatus;
    use serde_json::Value;

    #[test]
    fn degraded_sorts_between_up_and_down_so_the_worst_component_decides_the_whole() {
        assert!(HealthStatus::Degraded > HealthStatus::Up);
        assert!(HealthStatus::Down > HealthStatus::Up);
        assert!(HealthStatus::Down > HealthStatus::Degraded);
    }

    #[test]
    fn a_status_serialises_lowercase() {
        pretty_assertions::assert_eq!(serde_json::to_value(HealthStatus::Up).expect("serialises"), Value::String("up".to_owned()));
        pretty_assertions::assert_eq!(
            serde_json::to_value(HealthStatus::Degraded).expect("serialises"),
            Value::String("degraded".to_owned())
        );
        pretty_assertions::assert_eq!(serde_json::to_value(HealthStatus::Down).expect("serialises"), Value::String("down".to_owned()));
    }
}
