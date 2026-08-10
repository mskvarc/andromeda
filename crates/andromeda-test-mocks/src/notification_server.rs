//! HTTP notification callback server.

use crate::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, CallbackServerError, CapturedRequest, MockResponse, RequestMatcher},
    server_config::MockServerConfig,
};
use bytes::Bytes;
use http::{Method, StatusCode};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

/// Isolated HTTP notification receiver with raw request capture.
pub struct NotificationServer {
    path: CallbackPath,
    server: CallbackServer,
}

impl NotificationServer {
    /// Bind a notification endpoint returning `200 OK` for every delivery.
    pub async fn start(config: MockServerConfig) -> Result<NotificationServer, CallbackServerError> {
        let path = CallbackPath::parse(format!("/notify-{}", Uuid::new_v4()))?;
        let server = CallbackServer::start(config).await?;
        server
            .enqueue(
                RequestMatcher::new(Method::POST, path.clone()), // clone: the server owns its endpoint path after the matcher takes a copy.
                MockResponse::builder().status(StatusCode::OK).body(Bytes::new()).build(),
            )
            .await;
        Ok(NotificationServer { path, server })
    }

    /// URL the broker uses for notification delivery.
    pub fn endpoint(&self) -> Result<Url, CallbackServerError> {
        self.server.endpoint(&self.path)
    }

    /// URL text suitable for embedding in a notification endpoint payload.
    pub fn endpoint_url(&self) -> Result<String, CallbackServerError> {
        self.endpoint().map(|url| url.to_string())
    }

    /// Wait for the next raw notification request.
    pub async fn next_request(&self, timeout: Duration) -> Result<CapturedRequest, CallbackServerError> {
        self.server.next_request(timeout).await
    }

    /// Remove and return every notification request already captured without waiting.
    pub async fn drain_requests(&self) -> Vec<CapturedRequest> {
        self.server.drain_requests().await
    }

    /// Shut down the notification listener.
    pub async fn shutdown(self) -> Result<(), CallbackServerError> {
        self.server.shutdown().await
    }
}
