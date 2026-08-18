//! Typed, state-complete metrics route inventory.

use crate::{error::TelemetryHttpError, exposition::PrometheusText, handler, state::TelemetryRouteState};
use andromeda_telemetry::recorder::TelemetryRecorder;
use axum::{Router, routing::get};
use std::sync::Arc;

/// The complete metrics route fragment.
pub struct TelemetryRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl TelemetryRoutes {
    /// Construct `GET /metrics` over the installed recorder.
    pub fn new(recorder: Arc<TelemetryRecorder>) -> Result<TelemetryRoutes, TelemetryHttpError> {
        let router = Router::new()
            .route("/metrics", get(handler::scrape))
            .with_state(TelemetryRouteState::new(recorder, PrometheusText::content_type()?));
        Ok(TelemetryRoutes { router })
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::routes::TelemetryRoutes;
    use andromeda_telemetry::{names::QUEUE_OUTCOMES_TOTAL, recorder::TelemetryRecorder};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header::CONTENT_TYPE},
    };
    use metrics::counter;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    #[tokio::test]
    async fn the_metrics_path_renders_recorded_series_as_prometheus_text() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "unit", "outcome" => "completed").increment(4));
        let response = TelemetryRoutes::new(Arc::new(recorder))?
            .into_router()
            .oneshot(Request::get("/metrics").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        pretty_assertions::assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("text/plain; version=0.0.4")
        );
        let body = String::from_utf8(to_bytes(response.into_body(), usize::MAX).await?.to_vec())?;
        assert!(
            body.contains(&format!("{QUEUE_OUTCOMES_TOTAL}{{queue=\"unit\",outcome=\"completed\"}} 4")),
            "{body}"
        );
        Ok(())
    }
}
