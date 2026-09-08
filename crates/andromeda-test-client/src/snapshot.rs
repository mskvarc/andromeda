//! Snapshot endpoint operations.

use crate::{header::jsonld_link_header, response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::{APPLICATION_LD_JSON, ResponseMediaType};
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;
use urlencoding::encode;

impl TestClient {
    /// Create a snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn create_snapshot(&self, body: &Value) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join("snapshots")?;
        let request = self
            .request(Method::POST, url.clone()) // clone: transport diagnostics retain the request URL.
            .header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string())
            .json(body);
        self.send(Method::POST, url, request).await.map_err(RequestError::Transport)
    }

    /// Retrieve a snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`RequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn retrieve_snapshot(&self, id: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("snapshots/{}", encode(id)))?;
        let mut request = self.request(Method::GET, url.clone()).header("accept", ResponseMediaType::JsonLd.to_string()); // clone: transport diagnostics retain the request URL.
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::GET, url, request).await.map_err(RequestError::Transport)
    }

    /// Delete a snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn delete_snapshot(&self, id: &str) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("snapshots/{}", encode(id)))?;
        let request = self.request(Method::DELETE, url.clone()); // clone: transport diagnostics retain the request URL.
        self.send(Method::DELETE, url, request).await.map_err(RequestError::Transport)
    }
}
