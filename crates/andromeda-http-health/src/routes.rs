//! Typed, state-complete health route inventory.

use crate::{handler, state::HealthRouteState};
use andromeda_health::aggregate::Health;
use axum::{Router, routing::get};

/// Path of the management-plane readiness probe, at the root of the plane.
pub const READINESS_PATH: &str = "/readyz";

/// The complete health route fragment: the management plane's probes.
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

    /// Build the probe router.
    ///
    /// `GET /livez` is the dependency-free liveness probe and `GET /readyz`
    /// weighs the registered indicators and answers the full aggregate report,
    /// 503 when any is `Down`; these are the conventional Kubernetes probe
    /// paths, served on the bare management stack.
    pub fn into_router(self) -> Router {
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

    /// An indicator that always reports the status it was built with.
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

    #[tokio::test]
    async fn the_router_serves_livez_and_readyz() -> Result<(), Box<dyn Error>> {
        for path in ["/livez", "/readyz"] {
            let status = status_of(path, None).await?;
            assert_ne!(status, StatusCode::NOT_FOUND, "{path} was not routed");
        }
        Ok(())
    }

    #[tokio::test]
    async fn livez_is_dependency_free_and_returns_200() -> Result<(), Box<dyn Error>> {
        // A down indicator must not affect liveness: the process is still up.
        let status = status_of("/livez", Some(FixedIndicator { status: HealthStatus::Down })).await?;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn readyz_returns_503_when_an_indicator_is_down() -> Result<(), Box<dyn Error>> {
        let status = status_of("/readyz", Some(FixedIndicator { status: HealthStatus::Down })).await?;
        pretty_assertions::assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        Ok(())
    }

    #[tokio::test]
    async fn readyz_returns_200_when_up_or_degraded() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(status_of("/readyz", Some(FixedIndicator { status: HealthStatus::Up })).await?, StatusCode::OK);
        pretty_assertions::assert_eq!(
            status_of(
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
