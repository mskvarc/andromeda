//! Typed, state-complete health route inventory.

use crate::{handler, state::HealthRouteState};
use andromeda_health::aggregate::Health;
use axum::{Router, routing::get};

/// The complete health route fragment.
pub struct HealthRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl HealthRoutes {
    /// Construct the health routes with focused health state.
    ///
    /// `GET /health` and `GET /ready` return the full aggregate report, 503 when
    /// any indicator is `Down`; `GET /live` is a dependency-free liveness probe
    /// that always returns 200 while the process answers.
    pub fn new(health: Health) -> HealthRoutes {
        let router = Router::new()
            .route("/health", get(handler::check))
            .route("/ready", get(handler::check))
            .route("/live", get(handler::live))
            .with_state(HealthRouteState::new(health));
        HealthRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
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
    use std::{collections::BTreeMap, error::Error};
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
                details: BTreeMap::new(),
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
}
