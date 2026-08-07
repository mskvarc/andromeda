//! Running every registered indicator and reporting the worst answer.

use crate::{indicator::HealthIndicator, report::HealthCheckDetails, status::HealthStatus};
use std::{collections::BTreeMap, sync::Arc};

/// Aggregate health checker that runs all registered [`HealthIndicator`]s.
///
/// Cheap to clone: the indicators sit behind a shared `Arc`, so handing a copy
/// to a request handler costs one refcount rather than a re-registration.
#[derive(Clone)]
pub struct Health {
    /// Registered health indicators.
    indicators: Arc<Vec<Arc<dyn HealthIndicator>>>,
}

impl Health {
    /// Create a new [`HealthBuilder`] for registering indicators.
    pub const fn builder() -> HealthBuilder {
        HealthBuilder { indicators: Vec::new() }
    }

    /// Run all registered indicators and report the worst status alongside
    /// per-component details, in the [`HealthCheckDetails`] shape the response
    /// carries.
    ///
    /// Sequential rather than concurrent: a broker runs a handful of
    /// indicators, each of which is a round trip to a pool it already holds,
    /// and a `/health` call that fans out is a `/health` call that can be used
    /// to fan out. With no indicators registered the answer is
    /// [`HealthStatus::Up`] and an empty map — nothing to ask is not the same
    /// as nothing answering, but it is the only thing a broker with no
    /// subsystems can honestly say.
    pub async fn check_all(&self) -> HealthCheckDetails {
        let mut components = BTreeMap::new();
        let mut worst = HealthStatus::Up;

        for indicator in self.indicators.iter() {
            let detail = indicator.check().await;
            if detail.status > worst {
                worst = detail.status;
            }
            components.insert(indicator.name(), detail);
        }

        HealthCheckDetails { status: worst, components }
    }
}

/// Builder for constructing a [`Health`] instance with registered indicators.
pub struct HealthBuilder {
    /// Indicators collected so far.
    indicators: Vec<Arc<dyn HealthIndicator>>,
}

impl HealthBuilder {
    /// Register a health indicator.
    pub fn with_indicator(mut self, indicator: impl HealthIndicator) -> HealthBuilder {
        self.indicators.push(Arc::new(indicator));
        self
    }

    /// Build the [`Health`] instance.
    pub fn build(self) -> Health {
        Health {
            indicators: Arc::new(self.indicators),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        aggregate::Health,
        component::{ComponentDetail, ComponentName},
        indicator::HealthIndicator,
        status::HealthStatus,
    };
    use async_trait::async_trait;
    use std::{collections::BTreeMap, sync::Arc};

    /// An indicator that reports whatever it was constructed with.
    struct FixedIndicator {
        /// The name this indicator answers under.
        component: ComponentName,
        /// The status it always reports.
        status: HealthStatus,
    }

    #[async_trait]
    impl HealthIndicator for FixedIndicator {
        fn name(&self) -> ComponentName {
            self.component
        }

        async fn check(&self) -> ComponentDetail {
            ComponentDetail {
                status: self.status,
                details: BTreeMap::new(),
            }
        }
    }

    #[tokio::test]
    async fn a_broker_with_no_indicators_reports_up_and_names_no_components() {
        let health = Health::builder().build();
        let details = health.check_all().await;
        pretty_assertions::assert_eq!(details.status, HealthStatus::Up);
        assert!(details.components.is_empty());
    }

    #[tokio::test]
    async fn a_single_healthy_indicator_appears_under_its_own_name() {
        let health = Health::builder()
            .with_indicator(FixedIndicator {
                component: ComponentName::PostgreSql,
                status: HealthStatus::Up,
            })
            .build();

        let details = health.check_all().await;
        pretty_assertions::assert_eq!(details.status, HealthStatus::Up);
        pretty_assertions::assert_eq!(details.components[&ComponentName::PostgreSql].status, HealthStatus::Up);
    }

    #[tokio::test]
    async fn one_component_being_down_takes_the_whole_broker_down() {
        let health = Health::builder()
            .with_indicator(FixedIndicator {
                component: ComponentName::Mqtt,
                status: HealthStatus::Up,
            })
            .with_indicator(FixedIndicator {
                component: ComponentName::PostgreSql,
                status: HealthStatus::Down,
            })
            .build();

        let details = health.check_all().await;
        pretty_assertions::assert_eq!(details.status, HealthStatus::Down);
        pretty_assertions::assert_eq!(details.components[&ComponentName::Mqtt].status, HealthStatus::Up);
        pretty_assertions::assert_eq!(details.components[&ComponentName::PostgreSql].status, HealthStatus::Down);
    }

    #[test]
    fn cloning_shares_the_registered_indicators_rather_than_re_registering_them() {
        let health = Health::builder()
            .with_indicator(FixedIndicator {
                component: ComponentName::Mqtt,
                status: HealthStatus::Up,
            })
            .build();
        let cloned = health.clone(); // clone: the point of the test is that this is a refcount
        assert!(Arc::ptr_eq(&health.indicators, &cloned.indicators));
    }
}
