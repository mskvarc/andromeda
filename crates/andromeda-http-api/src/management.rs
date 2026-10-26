//! Assembly of the observability endpoints served on the management plane.
//!
//! The management plane carries only the conventional Kubernetes probes
//! (`/livez`, `/readyz`) and, when metrics are enabled, the Prometheus scrape
//! (`/metrics`), wrapped by the bare management middleware stack. It never
//! serves the NGSI-LD data-plane prefix, and never carries CORS, CSRF, helmet,
//! compression or the request-metric layer.

use andromeda_http_health::routes::HealthRoutes;
use andromeda_http_stack::stack::HttpStack;
use andromeda_http_telemetry::routes::TelemetryRoutes;
use axum::Router;
use tower_http::normalize_path::NormalizePath;

/// The management-plane route families.
///
/// The probes are always present; the metrics scrape is present only when the
/// operator enabled Prometheus exposure.
pub struct ManagementRoutes {
    /// Liveness and readiness probes.
    pub health: HealthRoutes,
    /// Metrics scrape route, present only when metrics exposure is enabled.
    pub metrics: Option<TelemetryRoutes>,
}

impl ManagementRoutes {
    /// Merge the management endpoints into a single root router.
    ///
    /// Shared by both the standalone management listener and the merged data
    /// plane, so neither reimplements the probe-plus-optional-metrics layout.
    pub(crate) fn into_root_router(self) -> Router {
        let router = self.health.into_management_router();
        match self.metrics {
            Some(metrics) => router.merge(metrics.into_router()),
            None => router,
        }
    }
}

/// Fully assembled management-plane HTTP service.
pub struct ManagementApi {
    /// Bare-stacked service with path normalization outside Axum routing.
    service: NormalizePath<Router>,
}

impl ManagementApi {
    /// Assemble the management endpoints under the bare management stack.
    #[must_use]
    pub fn assemble(routes: ManagementRoutes, stack: &HttpStack) -> ManagementApi {
        ManagementApi {
            service: stack.apply_management(routes.into_root_router()),
        }
    }

    /// Consume the API and return the generic state-complete HTTP service.
    #[must_use]
    pub fn into_service(self) -> NormalizePath<Router> {
        self.service
    }
}

#[cfg(test)]
mod tests {
    use crate::management::{ManagementApi, ManagementRoutes};
    use andromeda_configuration::schema::{
        feature_mode::HttpFeatureMode,
        http::{
            HttpStackConfig,
            cors::{CorsConfig, CorsMode},
            csrf::CsrfConfig,
            helmet::HelmetConfig,
        },
    };
    use andromeda_health::aggregate::Health;
    use andromeda_http_health::routes::HealthRoutes;
    use andromeda_http_stack::stack::HttpStack;
    use andromeda_http_telemetry::routes::TelemetryRoutes;
    use andromeda_telemetry::recorder::TelemetryRecorder;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    /// A stack with CORS and helmet enabled, to prove the management plane omits
    /// their headers even when the process-wide stack would carry them.
    fn stack_with_edge_features() -> Result<HttpStack, Box<dyn Error>> {
        Ok(HttpStack::new(HttpStackConfig {
            cors: CorsConfig {
                mode: CorsMode::Permissive,
                ..CorsConfig::default()
            },
            helmet: HelmetConfig {
                mode: HttpFeatureMode::Enabled,
            },
            csrf: CsrfConfig {
                mode: HttpFeatureMode::Disabled,
                trusted_origins: Vec::new(),
            },
            ..HttpStackConfig::default()
        })?)
    }

    fn management_routes(metrics: Option<TelemetryRoutes>) -> ManagementRoutes {
        ManagementRoutes {
            health: HealthRoutes::new(Health::builder().build()),
            metrics,
        }
    }

    async fn status_of(metrics: Option<TelemetryRoutes>, path: &str) -> Result<StatusCode, Box<dyn Error>> {
        let service = ManagementApi::assemble(management_routes(metrics), &stack_with_edge_features()?).into_service();
        let response = service.oneshot(Request::get(path).body(Body::empty())?).await?;
        Ok(response.status())
    }

    #[tokio::test]
    async fn management_service_serves_livez_and_readyz() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(status_of(None, "/livez").await?, StatusCode::OK);
        pretty_assertions::assert_eq!(status_of(None, "/readyz").await?, StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn management_service_serves_metrics_when_telemetry_supplied() -> Result<(), Box<dyn Error>> {
        let telemetry = TelemetryRoutes::new(Arc::new(TelemetryRecorder::detached()?))?;
        pretty_assertions::assert_eq!(status_of(Some(telemetry), "/metrics").await?, StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn management_service_omits_metrics_when_absent() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(status_of(None, "/metrics").await?, StatusCode::NOT_FOUND);
        Ok(())
    }

    #[tokio::test]
    async fn management_service_does_not_serve_the_ngsi_ld_prefix() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(status_of(None, "/ngsi-ld/v1/entities").await?, StatusCode::NOT_FOUND);
        Ok(())
    }

    #[tokio::test]
    async fn management_service_applies_no_cors_or_helmet_headers() -> Result<(), Box<dyn Error>> {
        let service = ManagementApi::assemble(management_routes(None), &stack_with_edge_features()?).into_service();
        let response = service
            .oneshot(Request::get("/livez").header("origin", "https://scraper.example").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !response.headers().contains_key("access-control-allow-origin"),
            "the bare management stack must not answer with a CORS header, found {:?}",
            response.headers()
        );
        assert!(
            !response.headers().contains_key("x-content-type-options"),
            "the bare management stack must not answer with a helmet header, found {:?}",
            response.headers()
        );
        Ok(())
    }
}
