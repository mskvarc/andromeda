//! Temporal Entity endpoint operations.

use crate::{header::jsonld_link_header, response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::{APPLICATION_LD_JSON, ResponseMediaType};
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;
use urlencoding::encode;

impl TestClient {
    /// Create a Temporal Entity.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn create_temporal_entity(&self, body: &Value) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join("temporal/entities")?;
        let request = self
            .request(Method::POST, url.clone()) // clone: transport diagnostics retain the request URL.
            .header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string())
            .json(body);
        self.send(Method::POST, url, request).await.map_err(RequestError::Transport)
    }

    /// Retrieve one Temporal Entity.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`RequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn get_temporal_entity(&self, id: &str, query: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.temporal_get(&format!("temporal/entities/{}{}", encode(id), query_suffix(query)), context)
            .await
    }

    /// Query Temporal Entities.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`RequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn query_temporal_entities(&self, query: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.temporal_get(&format!("temporal/entities{}", query_suffix(query)), context).await
    }

    /// Delete one Temporal Entity.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn delete_temporal_entity(&self, id: &str) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("temporal/entities/{}", encode(id)))?;
        let request = self.request(Method::DELETE, url.clone()); // clone: transport diagnostics retain the request URL.
        self.send(Method::DELETE, url, request).await.map_err(RequestError::Transport)
    }

    async fn temporal_get(&self, path: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(path)?;
        let mut request = self.request(Method::GET, url.clone()).header("accept", ResponseMediaType::JsonLd.to_string()); // clone: transport diagnostics retain the request URL.
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::GET, url, request).await.map_err(RequestError::Transport)
    }
}

fn query_suffix(query: &str) -> String {
    if query.is_empty() { String::new() } else { format!("?{query}") }
}
