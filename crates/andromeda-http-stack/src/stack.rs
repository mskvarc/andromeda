//! Deterministic application of API-wide and process-wide middleware.

use crate::{
    config::{CorsMode, HttpFeatureMode, HttpStackConfig},
    request_body_limit::RequestBodyLimit,
};
use andromeda_http_headers::{ngsild_tenant::NgsiLdTenant, x_request_id::XRequestId};
use andromeda_http_tenancy::propagation::echo_tenant_header;
use axum::{
    Extension,
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    http::{StatusCode, header},
    middleware,
    response::Response,
};
use axum_prometheus::{PrometheusMetricLayer, PrometheusMetricLayerBuilder};
use axum_server_timing::ServerTimingLayer;
use headers::{Header, HeaderMapExt};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tower::{Layer, ServiceBuilder};
use tower_helmet::HelmetLayer;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    decompression::RequestDecompressionLayer,
    normalize_path::{NormalizePath, NormalizePathLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

/// The process-wide axum request-metric layer, built at most once.
///
/// [`PrometheusMetricLayerBuilder::with_prefix`] sets a global prefix that may be
/// configured only once per process and panics on a second call, so the built
/// layer is memoised here and cloned into every stack the process assembles.
static HTTP_METRIC_LAYER: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Build (once) and clone the axum request-metric layer.
///
/// The layer emits `andromeda_http_requests_*` through the `metrics` facade into
/// whichever recorder the process installed; it registers no route and installs
/// no recorder of its own. Its ignore pattern excludes the scrape endpoint so a
/// scrape does not instrument itself.
fn http_metric_layer() -> PrometheusMetricLayer<'static> {
    HTTP_METRIC_LAYER
        .get_or_init(|| {
            PrometheusMetricLayerBuilder::new()
                .with_prefix("andromeda")
                .with_ignore_pattern("/ngsi-ld/v1/metrics")
                .build()
        })
        .clone()
}

/// Explicitly ordered inbound HTTP stack.
#[derive(Clone)]
pub struct HttpStack {
    /// Process-wide middleware configuration.
    config: HttpStackConfig,
    /// API-wide body admission limit.
    request_body_limit: RequestBodyLimit,
}

impl HttpStack {
    /// Construct an HTTP stack from its owner-defined configuration.
    #[must_use]
    pub const fn new(config: HttpStackConfig) -> HttpStack {
        let request_body_limit = RequestBodyLimit::new(config.max_request_body_bytes);
        HttpStack { config, request_body_limit }
    }

    /// Apply middleware scoped to the complete NGSI-LD API subtree.
    pub fn apply_api(&self, router: Router) -> Router {
        router
            // Every absolute URL the broker mints from a request origin reads
            // the operator's trust decision from this extension, so a router
            // assembled without the stack treats every forwarded-protocol field
            // as untrusted rather than inheriting a permissive default.
            .layer(Extension(self.config.forwarded_headers))
            .layer(DefaultBodyLimit::max(self.request_body_limit.bytes()))
            .layer(HelmetLayer::with_defaults())
    }

    /// Apply process-wide middleware in deterministic order and normalize paths before routing.
    pub fn apply_process(&self, router: Router) -> NormalizePath<Router> {
        let sensitive_request_headers = Arc::<[_]>::from([header::AUTHORIZATION, header::COOKIE, header::PROXY_AUTHORIZATION]);
        let sensitive_response_headers = Arc::<[_]>::from([header::SET_COOKIE]);
        let trace_layer = TraceLayer::new_for_http()
            .make_span_with(|request: &Request| {
                let path = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map_or_else(|| request.uri().path(), MatchedPath::as_str);
                let request_id = request
                    .extensions()
                    .get::<RequestId>()
                    .and_then(|id| id.header_value().to_str().ok())
                    .map(str::to_owned)
                    .or_else(|| request.headers().typed_get::<XRequestId>().map(|id| id.as_str().to_owned()))
                    .unwrap_or_default();
                let tenant = request
                    .headers()
                    .get(NgsiLdTenant::name())
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                tracing::info_span!(
                    "http-request",
                    method = %request.method(),
                    path = %path,
                    request_id = %request_id,
                    tenant = %tenant,
                )
            })
            .on_request(|request: &Request, _span: &tracing::Span| {
                let request_id = request
                    .extensions()
                    .get::<RequestId>()
                    .and_then(|id| id.header_value().to_str().ok())
                    .map(str::to_owned)
                    .or_else(|| request.headers().typed_get::<XRequestId>().map(|id| id.as_str().to_owned()))
                    .unwrap_or_default();
                tracing::info!(request_id = %request_id, method = %request.method(), uri = %request.uri(), "started request");
            })
            .on_response(|response: &Response, latency: Duration, _span: &tracing::Span| {
                let request_id = response
                    .headers()
                    .typed_get::<XRequestId>()
                    .map_or_else(String::new, |id| id.as_str().to_owned());
                tracing::info!(request_id = %request_id, status = %response.status(), latency_us = latency.as_micros(), "finished request");
            });
        let core = ServiceBuilder::new()
            // Outermost so the matched route is populated by the time the layer
            // records the response, giving the `endpoint` label its low-cardinality
            // `MatchedPath` value rather than the raw URI.
            .layer(http_metric_layer())
            .layer(SetSensitiveRequestHeadersLayer::from_shared(sensitive_request_headers))
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(trace_layer)
            .layer(SetSensitiveResponseHeadersLayer::from_shared(sensitive_response_headers))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(self.config.request_timeout_secs),
            ));
        let mut router = router.layer(middleware::from_fn(echo_tenant_header));
        if let HttpFeatureMode::Enabled = self.config.server_timing {
            router = router.layer(ServerTimingLayer::new("andromeda"));
        }
        if let HttpFeatureMode::Enabled = self.config.response_compression {
            router = router.layer(CompressionLayer::new());
        }
        if let HttpFeatureMode::Enabled = self.config.request_decompression {
            router = router.layer(RequestDecompressionLayer::new());
        }
        router = router.layer(core);
        if let CorsMode::Permissive = self.config.cors {
            router = router.layer(CorsLayer::permissive());
        }
        NormalizePathLayer::trim_trailing_slash().layer(router)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{CorsMode, HttpFeatureMode, HttpStackConfig},
        stack::HttpStack,
    };
    use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
    use axum::{
        Extension,
        Router,
        body::{Body, to_bytes},
        extract::{Request, State},
        http::StatusCode,
        middleware::{Next, from_fn_with_state},
        response::Response,
        routing::get,
    };
    use std::{error::Error, sync::Arc};
    use tokio::sync::Mutex;
    use tower::{Layer, ServiceExt};

    /// A stack configuration with every optional feature switched off.
    fn disabled_config() -> HttpStackConfig {
        HttpStackConfig {
            request_timeout_secs: 30,
            response_compression: HttpFeatureMode::Disabled,
            server_timing: HttpFeatureMode::Disabled,
            request_decompression: HttpFeatureMode::Disabled,
            cors: CorsMode::Disabled,
            max_request_body_bytes: 1024,
            forwarded_headers: ForwardedHeaderTrust::Untrusted,
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Phase {
        Outer,
        Api,
        Tenant,
        Local,
        Handler,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Direction {
        Inbound,
        Outbound,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Event {
        phase: Phase,
        direction: Direction,
    }

    #[derive(Clone)]
    struct Marker {
        phase: Phase,
        events: Arc<Mutex<Vec<Event>>>,
    }

    async fn mark(State(marker): State<Marker>, request: Request, next: Next) -> Response {
        marker.events.lock().await.push(Event {
            phase: marker.phase,
            direction: Direction::Inbound,
        });
        let response = next.run(request).await;
        marker.events.lock().await.push(Event {
            phase: marker.phase,
            direction: Direction::Outbound,
        });
        response
    }

    fn marker(phase: Phase, events: &Arc<Mutex<Vec<Event>>>) -> Marker {
        Marker {
            phase,
            events: Arc::clone(events),
        }
    }

    #[tokio::test]
    async fn scope_markers_run_in_declared_inbound_order_and_reverse_outbound_order() -> Result<(), Box<dyn Error>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let handler_events = Arc::clone(&events);
        let route = Router::new()
            .route(
                "/ordered",
                get(move || {
                    let events = Arc::clone(&handler_events);
                    async move {
                        events.lock().await.push(Event {
                            phase: Phase::Handler,
                            direction: Direction::Inbound,
                        });
                        StatusCode::OK
                    }
                }),
            )
            .route_layer(from_fn_with_state(marker(Phase::Local, &events), mark))
            .layer(from_fn_with_state(marker(Phase::Tenant, &events), mark))
            .layer(from_fn_with_state(marker(Phase::Api, &events), mark));
        let service = from_fn_with_state(marker(Phase::Outer, &events), mark).layer(route);
        service.oneshot(Request::get("/ordered").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(
            *events.lock().await,
            vec![
                Event {
                    phase: Phase::Outer,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Api,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Tenant,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Local,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Handler,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Local,
                    direction: Direction::Outbound
                },
                Event {
                    phase: Phase::Tenant,
                    direction: Direction::Outbound
                },
                Event {
                    phase: Phase::Api,
                    direction: Direction::Outbound
                },
                Event {
                    phase: Phase::Outer,
                    direction: Direction::Outbound
                },
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn route_local_policy_does_not_affect_unrelated_routes() -> Result<(), Box<dyn Error>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let strict = Router::new()
            .route("/strict", get(|| async { StatusCode::OK }))
            .route_layer(from_fn_with_state(marker(Phase::Local, &events), mark));
        let router = Router::new()
            .route("/entity", get(|| async { StatusCode::OK }))
            .route("/health", get(|| async { StatusCode::OK }))
            .merge(strict)
            .layer(from_fn_with_state(marker(Phase::Api, &events), mark));
        for path in ["/entity", "/health"] {
            events.lock().await.clear();
            router.clone().oneshot(Request::get(path).body(Body::empty())?).await?;
            assert!(events.lock().await.iter().all(|event| event.phase != Phase::Local));
        }
        events.lock().await.clear();
        router.oneshot(Request::get("/strict").body(Body::empty())?).await?;
        assert!(events.lock().await.iter().any(|event| event.phase == Phase::Local));
        Ok(())
    }

    /// The API stack hands every route the operator's trust decision, so the
    /// origin extractor never has to guess it.
    #[tokio::test]
    async fn the_api_stack_installs_the_configured_forwarded_header_trust() -> Result<(), Box<dyn Error>> {
        for declared in [ForwardedHeaderTrust::Untrusted, ForwardedHeaderTrust::Trusted] {
            let stack = HttpStack::new(HttpStackConfig {
                forwarded_headers: declared,
                ..disabled_config()
            });
            let router = stack.apply_api(Router::new().route(
                "/probe",
                get(|Extension(trust): Extension<ForwardedHeaderTrust>| async move { trust.to_string() }),
            ));
            let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            pretty_assertions::assert_eq!(String::from_utf8(body.to_vec())?, declared.to_string());
        }
        Ok(())
    }

    #[tokio::test]
    async fn process_stack_normalizes_paths_before_route_matching() -> Result<(), Box<dyn Error>> {
        let stack = HttpStack::new(disabled_config());
        let service = stack.apply_process(Router::new().route("/health", get(|| async { StatusCode::OK })));
        let response = service.oneshot(Request::get("/health/").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }
}
