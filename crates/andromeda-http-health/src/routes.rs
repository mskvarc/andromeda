//! Typed, state-complete health route inventory.

use crate::{handler, state::HealthRouteState};
use andromeda_health::aggregate::Health;
use axum::{Router, routing::get};

/// Path of the data-plane health report, relative to the NGSI-LD API root.
pub const HEALTH_PATH: &str = "/health";

/// Path of the management-plane readiness probe, at the root of the plane.
pub const READINESS_PATH: &str = "/readyz";

/// The complete health route fragment.
///
/// Holds the [`Health`] aggregate rather than a prebaked router so the same
/// registered indicators can back both the data-plane probes
/// ([`HealthRoutes::into_router`]) and the management-plane probes
/// ([`HealthRoutes::into_management_router`]).
#[derive(Clone)]
pub struct HealthRoutes {
    /// The registered health indicators backing every probe.
    health: Health,
}

impl HealthRoutes {
    /// Construct the health routes over the aggregate health capability.
    #[must_use]
    pub const fn new(health: Health) -> HealthRoutes {
        HealthRoutes { health }
    }

    /// Build the data-plane probe router.
    ///
    /// `GET /health` and `GET /ready` return the full aggregate report, 503 when
    /// any indicator is `Down`; `GET /live` is a dependency-free liveness probe
    /// that always returns 200 while the process answers.
    pub fn into_router(self) -> Router {
        Router::new()
            .route(HEALTH_PATH, get(handler::check))
            .route("/ready", get(handler::check))
            .route("/live", get(handler::live))
            .with_state(HealthRouteState::new(self.health))
    }

    /// Build the management-plane probe router.
    ///
    /// `GET /livez` is the dependency-free liveness probe and `GET /readyz`
    /// weighs the registered indicators, 503 when any is `Down`; these are the
    /// conventional Kubernetes probe paths, served on the bare management stack.
    pub fn into_management_router(self) -> Router {
        Router::new()
            .route("/livez", get(handler::live))
            .route(READINESS_PATH, get(handler::check))
            .with_state(HealthRouteState::new(self.health))
    }
}

#[cfg(test)]
mod tests {
    use crate::routes::HealthRoutes;
    use andromeda_health::{
        aggregate::Health,
        component::{ComponentDetail, ComponentName},
        indicator::HealthIndicator,
        status::HealthStatus,
    };
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::error::Error;
    use tower::ServiceExt;

    /// An indicator that always reports the status it was built with, and records
    /// whether it was consulted.
    struct FixedIndicator {
        /// The status it always reports.
        status: HealthStatus,
    }

    #[async_trait]
    impl HealthIndicator for FixedIndicator {
        fn name(&self) -> ComponentName {
            ComponentName::PostgreSql
        }

        async fn check(&self) -> ComponentDetail {
            ComponentDetail {
                status: self.status,
                details: None,
            }
        }
    }

    async fn status_of(path: &str, indicator: Option<FixedIndicator>) -> Result<StatusCode, Box<dyn Error>> {
        let mut builder = Health::builder();
        if let Some(indicator) = indicator {
            builder = builder.with_indicator(indicator);
        }
        let response = HealthRoutes::new(builder.build())
            .into_router()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        Ok(response.status())
    }

    async fn management_status_of(path: &str, indicator: Option<FixedIndicator>) -> Result<StatusCode, Box<dyn Error>> {
        let mut builder = Health::builder();
        if let Some(indicator) = indicator {
            builder = builder.with_indicator(indicator);
        }
        let response = HealthRoutes::new(builder.build())
            .into_management_router()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        Ok(response.status())
    }

    #[tokio::test]
    async fn health_returns_the_full_report_with_a_200_when_healthy() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(status_of("/health", None).await?, StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn live_returns_200_without_consulting_any_indicator() -> Result<(), Box<dyn Error>> {
        // A down indicator must not affect liveness: the process is still up.
        let status = status_of("/live", Some(FixedIndicator { status: HealthStatus::Down })).await?;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn ready_is_200_when_indicators_are_up_or_degraded() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(status_of("/ready", Some(FixedIndicator { status: HealthStatus::Up })).await?, StatusCode::OK);
        pretty_assertions::assert_eq!(
            status_of(
                "/ready",
                Some(FixedIndicator {
                    status: HealthStatus::Degraded
                })
            )
            .await?,
            StatusCode::OK
        );
        Ok(())
    }

    #[tokio::test]
    async fn ready_is_503_when_any_indicator_is_down() -> Result<(), Box<dyn Error>> {
        let status = status_of("/ready", Some(FixedIndicator { status: HealthStatus::Down })).await?;
        pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        Ok(())
    }

    #[tokio::test]
    async fn into_router_still_serves_health_ready_live() -> Result<(), Box<dyn Error>> {
        for path in ["/health", "/ready", "/live"] {
            let response = HealthRoutes::new(Health::builder().build())
                .into_router()
                .oneshot(Request::get(path).body(Body::empty())?)
                .await?;
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{path} was not routed");
        }
        Ok(())
    }

    #[tokio::test]
    async fn into_management_router_serves_livez_and_readyz() -> Result<(), Box<dyn Error>> {
        for path in ["/livez", "/readyz"] {
            let status = management_status_of(path, None).await?;
            assert_ne!(status, StatusCode::NOT_FOUND, "{path} was not routed");
        }
        Ok(())
    }

    #[tokio::test]
    async fn livez_is_dependency_free_and_returns_200() -> Result<(), Box<dyn Error>> {
        // A down indicator must not affect liveness: the process is still up.
        let status = management_status_of("/livez", Some(FixedIndicator { status: HealthStatus::Down })).await?;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn readyz_returns_503_when_an_indicator_is_down() -> Result<(), Box<dyn Error>> {
        let status = management_status_of("/readyz", Some(FixedIndicator { status: HealthStatus::Down })).await?;
        pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        Ok(())
    }

    #[tokio::test]
    async fn readyz_returns_200_when_up_or_degraded() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(
            management_status_of("/readyz", Some(FixedIndicator { status: HealthStatus::Up })).await?,
            StatusCode::OK
        );
        pretty_assertions::assert_eq!(
            management_status_of(
                "/readyz",
                Some(FixedIndicator {
                    status: HealthStatus::Degraded
                })
            )
            .await?,
            StatusCode::OK
        );
        Ok(())
    }
}
