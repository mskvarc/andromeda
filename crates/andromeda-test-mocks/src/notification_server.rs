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
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::Bind`] when the configured bind address cannot be claimed,
    /// [`CallbackServerError::LocalAddress`] when the address of the bound listener cannot be read, and
    /// [`CallbackServerError::Endpoint`] when the broker-visible host and the bound port do not form a valid URL.
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
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::Endpoint`] when the generated notification path cannot be joined onto the server endpoint URL.
    pub fn endpoint(&self) -> Result<Url, CallbackServerError> {
        self.server.endpoint(&self.path)
    }

    /// URL text suitable for embedding in a notification endpoint payload.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::Endpoint`] when the generated notification path cannot be joined onto the server endpoint URL.
    pub fn endpoint_url(&self) -> Result<String, CallbackServerError> {
        self.endpoint().map(|url| url.to_string())
    }

    /// Wait for the next raw notification request.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::RequestTimeout`] when no request is captured before `timeout` elapses.
    pub async fn next_request(&self, timeout: Duration) -> Result<CapturedRequest, CallbackServerError> {
        self.server.next_request(timeout).await
    }

    /// Remove and return every notification request already captured without waiting.
    pub async fn drain_requests(&self) -> Vec<CapturedRequest> {
        self.server.drain_requests().await
    }

    /// Shut down the notification listener.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::ShutdownSignal`] when the server task ended before the signal was delivered,
    /// [`CallbackServerError::Join`] when the server task panicked or was aborted, and
    /// [`CallbackServerError::Serve`] when Axum reported a serving failure.
    pub async fn shutdown(self) -> Result<(), CallbackServerError> {
        self.server.shutdown().await
    }
}
