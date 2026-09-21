//! Installation of the per-phase `Server-Timing` mechanism.

use crate::config::{
    feature_mode::HttpFeatureMode,
    server_timing::{ServerTimingConfig, ServerTimingPhase},
};
use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::Response,
};
use axum_server_timing::{ServerTiming, ServerTimingExtension, ServerTimingLayer};
use std::{
    sync::{MutexGuard, PoisonError},
    time::Instant,
};

/// The metric name the base layer records the whole-request span under.
const TOTAL_METRIC: &str = "total";
/// The metric name for the stack-entry-to-dispatch span.
const DISPATCH_METRIC: &str = "dispatch";

/// Install the `Server-Timing` mechanism the configuration calls for.
///
/// When enabled, the base [`ServerTimingLayer`] injects the shared response
/// extension and records the whole-request span under `total`; that total is
/// therefore emitted whenever `Server-Timing` is on. The `dispatch` phase adds a
/// middleware, installed inside the layer, that records the stack-entry-to-handler
/// span into the same extension. Deeper per-subsystem spans are recorded by their
/// owning crates into this extension and are not installed here.
pub(crate) fn apply_server_timing(router: Router, config: &ServerTimingConfig) -> Router {
    if let HttpFeatureMode::Disabled = config.mode {
        return router;
    }
    // The dispatch middleware must run inside the timing layer so the extension
    // the layer injects is already present when the middleware records into it.
    let router = if config.phases.contains(ServerTimingPhase::Dispatch) {
        router.layer(middleware::from_fn(record_dispatch_phase))
    } else {
        router
    };
    router.layer(ServerTimingLayer::new(TOTAL_METRIC))
}

/// Record the stack-entry-to-dispatch span into the shared timing extension.
async fn record_dispatch_phase(request: Request, next: Next) -> Response {
    let extension = request.extensions().get::<ServerTimingExtension>().cloned();
    let started = Instant::now();
    let response = next.run(request).await;
    if let Some(extension) = extension {
        let elapsed = started.elapsed();
        lock_recovering(&extension).record_timing(DISPATCH_METRIC.to_owned(), elapsed, None);
    }
    response
}

/// Lock the shared timing state, recovering the guard if a prior holder panicked.
///
/// The timing data is advisory, so a poisoned mutex is recovered rather than
/// propagated: a request that failed while recording must not stop the next one
/// from timing itself.
fn lock_recovering(extension: &ServerTimingExtension) -> MutexGuard<'_, ServerTiming> {
    extension.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{
            feature_mode::HttpFeatureMode,
            server_timing::{ServerTimingConfig, ServerTimingPhase},
        },
        stack::server_timing::apply_server_timing,
    };
    use axum::{Router, body::Body, extract::Request, http::StatusCode, routing::get};
    use std::error::Error;
    use tower::ServiceExt;

    fn probe_router(config: &ServerTimingConfig) -> Router {
        apply_server_timing(Router::new().route("/probe", get(|| async { StatusCode::OK })), config)
    }

    #[tokio::test]
    async fn server_timing_records_multiple_named_entries() -> Result<(), Box<dyn Error>> {
        let config = ServerTimingConfig {
            mode: HttpFeatureMode::Enabled,
            phases: ServerTimingPhase::Total | ServerTimingPhase::Dispatch,
        };
        let response = probe_router(&config).oneshot(Request::get("/probe").body(Body::empty())?).await?;
        let header = response
            .headers()
            .get("server-timing")
            .expect("server-timing header present")
            .to_str()?
            .to_owned();
        // A `Server-Timing` header lists its metrics comma-separated; the total and
        // the dispatch span make at least two.
        let metric_count = header.split(',').filter(|metric| !metric.trim().is_empty()).count();
        assert!(metric_count >= 2, "expected multiple metrics, found `{header}`");
        Ok(())
    }

    #[tokio::test]
    async fn disabled_server_timing_emits_no_header() -> Result<(), Box<dyn Error>> {
        let config = ServerTimingConfig {
            mode: HttpFeatureMode::Disabled,
            phases: ServerTimingPhase::Total.into(),
        };
        let response = probe_router(&config).oneshot(Request::get("/probe").body(Body::empty())?).await?;
        assert!(response.headers().get("server-timing").is_none());
        Ok(())
    }
}
