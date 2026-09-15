//! Test-boundary adapter retaining concise distributed-operation setup syntax.

use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CapturedRequest, MockResponse},
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
};
use serde_json::Value;
use std::time::Duration;

/// Mock Context Source that records the requests a broker forwards to it and replies with stubs.
pub struct ContextSourceAdapter {
    server: ContextSourceServer,
}

impl ContextSourceAdapter {
    /// Starts a Context Source server bound to the mock server address from the environment.
    ///
    /// # Panics
    ///
    /// Panics when the mock server configuration cannot be read from the environment, or when the
    /// Context Source server fails to bind its listening socket.
    pub async fn start() -> ContextSourceAdapter {
        let configuration = MockServerConfig::from_environment().expect("valid Context Source server configuration");
        let server = ContextSourceServer::start(configuration).await.expect("Context Source server starts");
        ContextSourceAdapter { server }
    }

    /// Queues a bodiless response for the next request matching `method` and `path`.
    ///
    /// # Panics
    ///
    /// Panics when `path` is not an absolute path or `status_code` is not a valid HTTP status.
    pub async fn set_stub(&self, method: Method, path: &str, status_code: u16) {
        self.enqueue(method, path, status_code, None, None).await;
    }

    /// Queues a JSON-LD response for the next request matching `method` and `path`.
    ///
    /// # Panics
    ///
    /// Panics when `body` cannot be serialized, when `path` is not an absolute path, or when
    /// `status_code` is not a valid HTTP status.
    pub async fn set_stub_with_body(&self, method: Method, path: &str, status_code: u16, body: &Value) {
        let body = serde_json::to_vec(body).expect("Context Source JSON response serializes");
        self.enqueue(method, path, status_code, Some(Bytes::from(body)), Some("application/ld+json"))
            .await;
    }

    /// Queues a response with a verbatim body and content type, for malformed-payload scenarios.
    ///
    /// # Panics
    ///
    /// Panics when `path` is not an absolute path, when `status_code` is not a valid HTTP status,
    /// or when `content_type` is not a valid header value.
    pub async fn set_stub_with_raw_body(&self, method: Method, path: &str, status_code: u16, body: &str, content_type: &str) {
        self.enqueue(method, path, status_code, Some(Bytes::copy_from_slice(body.as_bytes())), Some(content_type))
            .await;
    }

    async fn enqueue(&self, method: Method, path: &str, status_code: u16, body: Option<Bytes>, content_type: Option<&str>) {
        let path = CallbackPath::parse(path).expect("Context Source stub path is absolute");
        let status = StatusCode::from_u16(status_code).expect("Context Source response status is valid");
        let mut headers = HeaderMap::new();
        if let Some(content_type) = content_type {
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(content_type).expect("Context Source content type is valid"));
        }
        self.server
            .enqueue(
                method,
                path,
                MockResponse::builder().status(status).headers(headers).body(body.unwrap_or_default()).build(),
            )
            .await;
    }

    /// Returns how many requests matching `method` and `path` the broker has forwarded so far.
    ///
    /// # Panics
    ///
    /// Panics when `path` is not an absolute path.
    #[must_use]
    pub fn get_count(&self, method: Method, path: &str) -> usize {
        let path = CallbackPath::parse(path).expect("Context Source count path is absolute");
        self.server.request_count(method, path)
    }

    /// Returns how many requests of any kind the broker has forwarded so far.
    #[must_use]
    pub fn get_total_count(&self) -> usize {
        self.server.total_request_count()
    }

    /// Take the next forwarded request, so its method, path and query string can be asserted on.
    ///
    /// # Panics
    ///
    /// Panics when no request is forwarded within `timeout`.
    pub async fn next_request(&self, timeout: Duration) -> CapturedRequest {
        self.server.next_request(timeout).await.expect("Context Source captured a forwarded request")
    }

    /// Take every request captured so far, in arrival order, clearing the queue.
    pub async fn drain_requests(&self) -> Vec<CapturedRequest> {
        self.server.drain_requests().await
    }

    /// Returns the absolute URL a registration should point at to reach `path` on this source.
    ///
    /// # Panics
    ///
    /// Panics when `path` is not an absolute path, or when the server's bound address cannot be
    /// turned into an endpoint URL.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        let path = if path.is_empty() { "/" } else { path };
        let path = CallbackPath::parse(path).expect("Context Source endpoint path is absolute");
        self.server.endpoint(&path).expect("Context Source endpoint resolves").to_string()
    }

    /// Shuts the Context Source server down and releases its listening socket.
    ///
    /// # Panics
    ///
    /// Panics when the server task fails to terminate cleanly.
    pub async fn shutdown(self) {
        self.server.shutdown().await.expect("Context Source server shuts down");
    }
}
