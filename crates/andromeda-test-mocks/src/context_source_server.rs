//! Programmable Context Source forwarding server.

use crate::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, CallbackServerError, CapturedRequest, MockResponse, RequestMatcher},
    server_config::MockServerConfig,
};
use http::Method;
use std::time::Duration;
use url::Url;

/// Context Source server configured with exact request match rules.
pub struct ContextSourceServer {
    server: CallbackServer,
}

impl ContextSourceServer {
    /// Bind a ready Context Source server.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::Bind`] when the configured bind address cannot be claimed,
    /// [`CallbackServerError::LocalAddress`] when the address of the bound listener cannot be read, and
    /// [`CallbackServerError::Endpoint`] when the broker-visible host and the bound port do not form a valid URL.
    pub async fn start(config: MockServerConfig) -> Result<ContextSourceServer, CallbackServerError> {
        Ok(ContextSourceServer {
            server: CallbackServer::start(config).await?,
        })
    }

    /// Queue a response for an exact method and path.
    pub async fn enqueue(&self, method: Method, path: CallbackPath, response: MockResponse) {
        self.server.enqueue(RequestMatcher::new(method, path), response).await;
    }

    /// Resolve a Context Source resource URL.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::Endpoint`] when `path` cannot be joined onto the server endpoint URL.
    pub fn endpoint(&self, path: &CallbackPath) -> Result<Url, CallbackServerError> {
        self.server.endpoint(path)
    }

    /// Wait for the next forwarded request.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackServerError::RequestTimeout`] when no request is captured before `timeout` elapses.
    pub async fn next_request(&self, timeout: Duration) -> Result<CapturedRequest, CallbackServerError> {
        self.server.next_request(timeout).await
    }

    /// Return the number of requests captured for one exact method and path.
    #[must_use]
    pub fn request_count(&self, method: Method, path: CallbackPath) -> usize {
        self.server.request_count(&RequestMatcher::new(method, path))
    }

    /// Return the total number of requests captured by this server.
    #[must_use]
    pub fn total_request_count(&self) -> usize {
        self.server.total_request_count()
    }

    /// Shut down the Context Source listener.
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
