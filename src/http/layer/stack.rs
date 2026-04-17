//! Shared HTTP middleware stack for broker routes.

use crate::{
    core::config::{CorsMode, HttpConfig, HttpFeatureMode},
    http::header::ngsild_tenant::NgsiLdTenant,
};
use axum::{
    Router,
    extract::MatchedPath,
    http::{Request, StatusCode, header},
};
use headers::Header;
use std::{sync::Arc, time::Duration};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    decompression::RequestDecompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

/// Apply the shared HTTP middleware stack to an Axum router.
pub(crate) fn apply_http_middleware<S>(router: Router<S>, config: &HttpConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
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
                .and_then(|request_id| request_id.header_value().to_str().ok())
                .or_else(|| request.headers().get("x-request-id").and_then(|request_id| request_id.to_str().ok()))
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
                .and_then(|request_id| request_id.header_value().to_str().ok())
                .or_else(|| request.headers().get("x-request-id").and_then(|request_id| request_id.to_str().ok()))
                .unwrap_or_default();
            tracing::info!(
                request_id = %request_id,
                method = %request.method(),
                uri = %request.uri(),
                "started request",
            );
        })
        .on_response(|response: &axum::http::Response<_>, latency: Duration, _span: &tracing::Span| {
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|request_id| request_id.to_str().ok())
                .unwrap_or_default();
            tracing::info!(
                request_id = %request_id,
                status = %response.status(),
                latency_us = latency.as_micros(),
                "finished request",
            );
        });
    let router = match config.response_compression {
        HttpFeatureMode::Disabled => router,
        HttpFeatureMode::Enabled => router.layer(CompressionLayer::new()),
    };
    let router = match config.request_decompression {
        HttpFeatureMode::Disabled => router,
        HttpFeatureMode::Enabled => router.layer(RequestDecompressionLayer::new()),
    };
    let router = router.layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(config.request_timeout_secs),
    ));
    let router = router.layer(PropagateRequestIdLayer::x_request_id());
    let router = router.layer(SetSensitiveResponseHeadersLayer::from_shared(sensitive_response_headers));
    let router = router.layer(trace_layer);
    let router = router.layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));
    let router = router.layer(SetSensitiveRequestHeadersLayer::from_shared(sensitive_request_headers));

    match config.cors {
        CorsMode::Disabled => router,
        CorsMode::Permissive => router.layer(CorsLayer::permissive()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use axum::{
        body::{Body, Bytes, to_bytes},
        extract::DefaultBodyLimit,
        http::{Method, Request},
        response::IntoResponse,
        routing::{get, post},
    };
    use flate2::{Compression, read::GzDecoder, write::GzEncoder};
    use serial_test::serial;
    use std::{
        io::{Read, Write},
        sync::{Arc, Mutex},
    };
    use tokio::time::sleep;
    use tower::ServiceExt;
    use tracing_subscriber::fmt;

    fn test_http_config() -> HttpConfig {
        HttpConfig {
            request_timeout_secs: 30,
            response_compression: HttpFeatureMode::Enabled,
            request_decompression: HttpFeatureMode::Enabled,
            cors: CorsMode::Permissive,
        }
    }

    fn gzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).expect("gzip input should encode");
        encoder.finish().expect("gzip encoder should finish")
    }

    fn gunzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut decoder = GzDecoder::new(input);
        let mut output = Vec::new();
        decoder.read_to_end(&mut output).expect("gzip body should decode");
        output
    }

    #[test]
    fn request_id_is_generated_when_missing() {
        let app = apply_http_middleware(Router::new().route("/health", get(|| async { StatusCode::OK })), &test_http_config());

        let response = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async {
                app.oneshot(Request::builder().uri("/health").body(Body::empty()).expect("request should build"))
                    .await
            })
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response.headers().get("x-request-id").expect("request id header should exist");
        assert!(!request_id.is_empty());
    }

    #[test]
    fn request_id_is_propagated_when_present() {
        let app = apply_http_middleware(Router::new().route("/health", get(|| async { StatusCode::OK })), &test_http_config());

        let response = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async {
                app.oneshot(
                    Request::builder()
                        .uri("/health")
                        .header("x-request-id", "test-request-id")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
            })
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("x-request-id").expect("request id header should exist"),
            "test-request-id"
        );
    }

    #[test]
    fn permissive_cors_preflight_returns_cors_headers() {
        let app = apply_http_middleware(Router::new().route("/entities", post(|| async { StatusCode::CREATED })), &test_http_config());

        let response = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async {
                app.oneshot(
                    Request::builder()
                        .method(Method::OPTIONS)
                        .uri("/entities")
                        .header(header::ORIGIN, "https://example.com")
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
            })
            .expect("preflight should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).expect("allow-origin should exist"),
            "*"
        );
        assert!(response.headers().contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
    }

    #[test]
    fn permissive_cors_actual_request_returns_cors_headers() {
        let app = apply_http_middleware(Router::new().route("/health", get(|| async { StatusCode::OK })), &test_http_config());

        let response = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async {
                app.oneshot(
                    Request::builder()
                        .uri("/health")
                        .header(header::ORIGIN, "https://example.com")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
            })
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).expect("allow-origin should exist"),
            "*"
        );
    }

    #[test]
    fn compressed_response_is_returned_for_gzip_clients() {
        let app = apply_http_middleware(
            Router::new().route("/payload", get(|| async { "andromeda".repeat(512).into_response() })),
            &test_http_config(),
        );

        let response = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async {
                app.oneshot(
                    Request::builder()
                        .uri("/payload")
                        .header(header::ACCEPT_ENCODING, "gzip")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
            })
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_ENCODING).expect("content encoding should exist"), "gzip");
        let body = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async { to_bytes(response.into_body(), usize::MAX).await })
            .expect("response body should collect");
        let decoded = gunzip_bytes(body.as_ref());
        assert_eq!(decoded, "andromeda".repeat(512).as_bytes());
    }

    #[test]
    fn compressed_request_body_is_decompressed_for_handler() {
        let app = apply_http_middleware(Router::new().route("/echo", post(|body: Bytes| async move { body })), &test_http_config());
        let compressed_body = gzip_bytes(b"compressed-payload");

        let response = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async {
                app.oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/echo")
                        .header(header::CONTENT_ENCODING, "gzip")
                        .body(Body::from(compressed_body))
                        .expect("request should build"),
                )
                .await
            })
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body = tokio::runtime::Runtime::new()
            .expect("runtime should build")
            .block_on(async { to_bytes(response.into_body(), usize::MAX).await })
            .expect("response body should collect");
        assert_eq!(body.as_ref(), b"compressed-payload");
    }

    #[tokio::test]
    async fn timeout_returns_request_timeout_response() {
        let config = HttpConfig {
            request_timeout_secs: 1,
            ..test_http_config()
        };
        let app = apply_http_middleware(
            Router::new().route(
                "/slow",
                get(|| async {
                    sleep(Duration::from_secs(2)).await;
                    StatusCode::OK
                }),
            ),
            &config,
        );

        let response = app
            .oneshot(Request::builder().uri("/slow").body(Body::empty()).expect("request should build"))
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn body_limit_still_applies_after_request_decompression() {
        let app = apply_http_middleware(
            Router::new()
                .route("/limited", post(|body: Bytes| async move { body.len().to_string() }))
                .layer(DefaultBodyLimit::max(8)),
            &test_http_config(),
        );
        let compressed_body = gzip_bytes(b"body larger than eight bytes");

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/limited")
                    .header(header::CONTENT_ENCODING, "gzip")
                    .body(Body::from(compressed_body))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    #[serial]
    fn trace_logs_include_request_id_without_logging_secrets() {
        #[derive(Clone)]
        struct BufferWriter {
            buffer: Arc<Mutex<Vec<u8>>>,
        }

        impl Write for BufferWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.buffer.lock().expect("log buffer should lock").extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let logs = Arc::new(Mutex::new(Vec::new()));
        let make_writer = {
            let logs = Arc::clone(&logs);
            move || BufferWriter { buffer: Arc::clone(&logs) }
        };
        let subscriber = fmt::Subscriber::builder().with_writer(make_writer).with_ansi(false).without_time().finish();
        let dispatch = tracing::Dispatch::new(subscriber);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let response = tracing::dispatcher::with_default(&dispatch, || {
            runtime.block_on(async {
                let app = apply_http_middleware(Router::new().route("/health", get(|| async { StatusCode::OK })), &test_http_config());

                app.oneshot(
                    Request::builder()
                        .uri("/health")
                        .header("x-request-id", "trace-request-id")
                        .header(header::AUTHORIZATION, "Bearer secret-token")
                        .header(header::COOKIE, "sid=secret")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
            })
        })
        .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let rendered = String::from_utf8(logs.lock().expect("log buffer should lock").clone()).expect("logs should be utf8");
        assert!(rendered.contains("trace-request-id"), "{rendered}");
        assert!(!rendered.contains("secret-token"), "{rendered}");
        assert!(!rendered.contains("sid=secret"), "{rendered}");
    }
}
