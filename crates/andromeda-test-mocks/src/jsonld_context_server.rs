//! Hosted JSON-LD context server with configurable caching headers.

use crate::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, CallbackServerError, CapturedRequest, MockResponse, RequestMatcher},
    server_config::MockServerConfig,
};
use andromeda_http_headers::media_type::APPLICATION_LD_JSON;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// JSON-LD context document server.
pub struct JsonLdContextServer {
    server: CallbackServer,
}

impl JsonLdContextServer {
    /// Bind a ready JSON-LD context server.
    pub async fn start(config: MockServerConfig) -> Result<JsonLdContextServer, CallbackServerError> {
        Ok(JsonLdContextServer {
            server: CallbackServer::start(config).await?,
        })
    }

    /// Serve a context body and caller-selected cache headers at one path.
    pub async fn serve(&self, path: CallbackPath, body: Bytes, mut headers: HeaderMap) -> Result<Url, JsonLdContextServerError> {
        let content_type = HeaderValue::from_str(&APPLICATION_LD_JSON.to_string()).map_err(JsonLdContextServerError::ContentType)?;
        headers.insert(CONTENT_TYPE, content_type);
        self.server
            .enqueue(
                RequestMatcher::new(Method::GET, path.clone()), // clone: the response queue and returned endpoint both retain the path.
                MockResponse::builder().status(StatusCode::OK).headers(headers).body(body).build(),
            )
            .await;
        self.server.endpoint(&path).map_err(JsonLdContextServerError::Server)
    }

    /// Wait for the next context fetch.
    pub async fn next_request(&self, timeout: Duration) -> Result<CapturedRequest, CallbackServerError> {
        self.server.next_request(timeout).await
    }

    /// Return the number of context fetches captured for one path.
    pub fn request_count(&self, path: &CallbackPath) -> usize {
        self.server.request_count(&RequestMatcher::new(Method::GET, path.clone())) // clone: the temporary matcher owns its path key.
    }

    /// Shut down the context listener.
    pub async fn shutdown(self) -> Result<(), CallbackServerError> {
        self.server.shutdown().await
    }
}

/// JSON-LD context server configuration failure.
#[derive(Debug, Error)]
pub enum JsonLdContextServerError {
    /// Shared callback server failure.
    #[error(transparent)]
    Server(#[from] CallbackServerError),
    /// The typed JSON-LD media type could not be encoded as an HTTP header.
    #[error("JSON-LD media type cannot be encoded as an HTTP header: {0}")]
    ContentType(#[source] http::header::InvalidHeaderValue),
}
