//! Core health check abstractions.
//!
//! Provides the [`HealthIndicator`] trait for individual component checks and
//! the [`Health`] aggregate that runs all registered indicators and reports the
//! worst-case status.

pub mod database;
pub mod mqtt;

use andromeda_domain::health::{ComponentDetail, ComponentName, HealthStatus};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

/// A single component health check.
///
/// Implementors report their component name and produce a [`ComponentDetail`]
/// describing current health.
pub trait HealthIndicator: Send + Sync + 'static {
    /// Component name used as the key in the health response.
    fn name(&self) -> ComponentName;

    /// Perform the health check and return component details.
    fn check(&self) -> Pin<Box<dyn Future<Output = ComponentDetail> + Send + '_>>;
}

/// Aggregate health checker that runs all registered [`HealthIndicator`]s.
#[derive(Clone)]
pub struct Health {
    /// Registered health indicators.
    indicators: Arc<Vec<Arc<dyn HealthIndicator>>>,
}

impl Health {
    /// Create a new [`HealthBuilder`] for registering indicators.
    pub fn builder() -> HealthBuilder {
        HealthBuilder { indicators: Vec::new() }
    }

    /// Run all registered indicators and return the worst status alongside
    /// per-component details.
    ///
    /// When no indicators are registered, returns [`HealthStatus::Up`] with an
    /// empty component map.
    pub async fn check_all(&self) -> (HealthStatus, BTreeMap<ComponentName, ComponentDetail>) {
        let mut components = BTreeMap::new();
        let mut worst = HealthStatus::Up;

        for indicator in self.indicators.iter() {
            let detail = indicator.check().await;
            if detail.status > worst {
                worst = detail.status;
            }
            components.insert(indicator.name(), detail);
        }

        (worst, components)
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
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    struct MockIndicator {
        component: ComponentName,
        status: HealthStatus,
    }

    impl HealthIndicator for MockIndicator {
        fn name(&self) -> ComponentName {
            self.component
        }

        fn check(&self) -> Pin<Box<dyn Future<Output = ComponentDetail> + Send + '_>> {
            let status = self.status;
            Box::pin(async move {
                ComponentDetail {
                    status,
                    details: BTreeMap::new(),
                }
            })
        }
    }

    #[tokio::test]
    async fn no_indicators_returns_up() {
        let health = Health::builder().build();
        let (status, components) = health.check_all().await;
        pretty_assertions::assert_eq!(status, HealthStatus::Up);
        assert!(components.is_empty());
    }

    #[tokio::test]
    async fn single_up_indicator() {
        let health = Health::builder()
            .with_indicator(MockIndicator {
                component: ComponentName::PostgreSql,
                status: HealthStatus::Up,
            })
            .build();

        let (status, components) = health.check_all().await;
        pretty_assertions::assert_eq!(status, HealthStatus::Up);
        assert!(components.contains_key(&ComponentName::PostgreSql));
        pretty_assertions::assert_eq!(components[&ComponentName::PostgreSql].status, HealthStatus::Up);
    }

    #[tokio::test]
    async fn worst_status_wins() {
        let health = Health::builder()
            .with_indicator(MockIndicator {
                component: ComponentName::Mqtt,
                status: HealthStatus::Up,
            })
            .with_indicator(MockIndicator {
                component: ComponentName::PostgreSql,
                status: HealthStatus::Down,
            })
            .build();

        let (status, components) = health.check_all().await;
        pretty_assertions::assert_eq!(status, HealthStatus::Down);
        assert!(components.contains_key(&ComponentName::Mqtt));
        assert!(components.contains_key(&ComponentName::PostgreSql));
        pretty_assertions::assert_eq!(components[&ComponentName::Mqtt].status, HealthStatus::Up);
        pretty_assertions::assert_eq!(components[&ComponentName::PostgreSql].status, HealthStatus::Down);
    }

    #[test]
    fn health_is_clone() {
        let health = Health::builder().build();
        let cloned = health.clone();
        // Both share same Arc -- just verify clone compiles and works.
        assert!(Arc::ptr_eq(&health.indicators, &cloned.indicators));
    }
}
