//! Test-boundary adapter retaining concise distributed-operation setup syntax.

use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::MockResponse,
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
};
use serde_json::Value;

pub struct ContextSourceAdapter {
    server: ContextSourceServer,
}

impl ContextSourceAdapter {
    pub async fn start() -> ContextSourceAdapter {
        let configuration = MockServerConfig::from_environment().expect("valid Context Source server configuration");
        let server = ContextSourceServer::start(configuration).await.expect("Context Source server starts");
        ContextSourceAdapter { server }
    }

    pub async fn set_stub(&self, method: Method, path: &str, status_code: u16) {
        self.enqueue(method, path, status_code, None, None).await;
    }

    pub async fn set_stub_with_body(&self, method: Method, path: &str, status_code: u16, body: &Value) {
        let body = serde_json::to_vec(body).expect("Context Source JSON response serializes");
        self.enqueue(method, path, status_code, Some(Bytes::from(body)), Some("application/ld+json"))
            .await;
    }

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

    pub async fn get_count(&self, method: Method, path: &str) -> usize {
        let path = CallbackPath::parse(path).expect("Context Source count path is absolute");
        self.server.request_count(method, path)
    }

    pub async fn get_total_count(&self) -> usize {
        self.server.total_request_count()
    }

    pub fn url(&self, path: &str) -> String {
        let path = if path.is_empty() { "/" } else { path };
        let path = CallbackPath::parse(path).expect("Context Source endpoint path is absolute");
        self.server.endpoint(&path).expect("Context Source endpoint resolves").to_string()
    }

    pub async fn shutdown(self) {
        self.server.shutdown().await.expect("Context Source server shuts down");
    }
}
