use crate::{
    api::{router::create_router, tenant::echo_tenant_header_middleware},
    core::{
        config::{CorsMode, HttpFeatureMode, RuntimeConfig},
        error::AndromedaError,
    },
};
use andromeda_ngsi_ld_http_headers::ngsild_tenant::NgsiLdTenant;
use axum::{
    ServiceExt,
    extract::MatchedPath,
    http::{Request, StatusCode, header},
    middleware,
};
use headers::Header;
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    decompression::RequestDecompressionLayer,
    normalize_path::NormalizePathLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tower_layer::Layer;
use tracing::info;

pub async fn run(config: RuntimeConfig) -> Result<(), AndromedaError> {
    let config = Arc::new(config);
    let http = &config.http;

    let sensitive_request_headers = Arc::<[_]>::from([header::AUTHORIZATION, header::COOKIE, header::PROXY_AUTHORIZATION]);
    let sensitive_response_headers = Arc::<[_]>::from([header::SET_COOKIE]);

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<_>| {
            let path = request
                .extensions()
                .get::<MatchedPath>()
                .map_or_else(|| request.uri().path(), MatchedPath::as_str);
            let request_id = request
                .extensions()
                .get::<RequestId>()
                .and_then(|id| id.header_value().to_str().ok())
                .or_else(|| request.headers().get("x-request-id").and_then(|id| id.to_str().ok()))
                .unwrap_or_default();
            let tenant = request
                .headers()
                .get(NgsiLdTenant::name())
                .and_then(|tenant| tenant.to_str().ok())
                .unwrap_or_default();

            tracing::info_span!(
                "http-request",
                method = %request.method(),
                path = %path,
                request_id = %request_id,
                tenant = %tenant,
            )
        })
        .on_request(|request: &Request<_>, _span: &tracing::Span| {
            let request_id = request
                .extensions()
                .get::<RequestId>()
                .and_then(|id| id.header_value().to_str().ok())
                .or_else(|| request.headers().get("x-request-id").and_then(|id| id.to_str().ok()))
                .unwrap_or_default();
            tracing::info!(
                request_id = %request_id,
                method = %request.method(),
                uri = %request.uri(),
                "started request",
            );
        })
        .on_response(|response: &axum::http::Response<_>, latency: Duration, _span: &tracing::Span| {
            let request_id = response.headers().get("x-request-id").and_then(|id| id.to_str().ok()).unwrap_or_default();
            tracing::info!(
                request_id = %request_id,
                status = %response.status(),
                latency_us = latency.as_micros(),
                "finished request",
            );
        });

    let core_middleware = ServiceBuilder::new()
        .layer(SetSensitiveRequestHeadersLayer::from_shared(sensitive_request_headers))
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(trace_layer)
        .layer(SetSensitiveResponseHeadersLayer::from_shared(sensitive_response_headers))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(http.request_timeout_secs),
        ));

    let mut router = create_router(Arc::clone(&config))
        .await?
        .layer(middleware::from_fn(echo_tenant_header_middleware));

    if let HttpFeatureMode::Enabled = http.response_compression {
        router = router.layer(CompressionLayer::new());
    }
    if let HttpFeatureMode::Enabled = http.request_decompression {
        router = router.layer(RequestDecompressionLayer::new());
    }
    router = router.layer(core_middleware);
    if let CorsMode::Permissive = http.cors {
        router = router.layer(CorsLayer::permissive());
    }

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let listener = TcpListener::bind(std::net::SocketAddr::from((config.server.host, config.server.port))).await?;

    info!("Listening on {}", listener.local_addr()?);

    axum::serve(listener, ServiceExt::<axum::extract::Request>::into_make_service(app)).await?;

    Ok(())
}
