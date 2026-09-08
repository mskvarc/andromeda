//! JSON-LD Context endpoint operations.

use crate::{response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::APPLICATION_JSON;
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;
use urlencoding::encode;

impl TestClient {
    /// Create a hosted JSON-LD context.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn create_jsonld_context(&self, body: &Value) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join("jsonldContexts")?;
        let request = self
            .request(Method::POST, url.clone()) // clone: transport diagnostics retain the request URL.
            .header(CONTENT_TYPE, APPLICATION_JSON.to_string())
            .json(body);
        self.send(Method::POST, url, request).await.map_err(RequestError::Transport)
    }

    /// Retrieve a JSON-LD context resource.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn retrieve_jsonld_context(&self, id: &str) -> Result<TestResponse, RequestError> {
        self.context_resource(Method::GET, id).await
    }

    /// Delete a JSON-LD context resource.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn delete_jsonld_context(&self, id: &str) -> Result<TestResponse, RequestError> {
        self.context_resource(Method::DELETE, id).await
    }

    async fn context_resource(&self, method: Method, id: &str) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("jsonldContexts/{}", encode(id)))?;
        let request = self.request(method.clone(), url.clone()); // clone: the builder and transport diagnostics each own method and URL context.
        self.send(method, url, request).await.map_err(RequestError::Transport)
    }
}
