//! Discovery and broker-information endpoint operations.

use crate::{header::jsonld_link_header, response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::ResponseMediaType;
use http::Method;

impl TestClient {
    /// List entity types.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`RequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn list_types(&self, query: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.discovery_get(&format!("types{}", query_suffix(query)), context).await
    }

    /// List attributes.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`RequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn list_attributes(&self, query: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.discovery_get(&format!("attributes{}", query_suffix(query)), context).await
    }

    /// Retrieve broker information from `/info`.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn info(&self) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join("info")?;
        let request = self.request(Method::GET, url.clone()); // clone: transport diagnostics retain the request URL.
        self.send(Method::GET, url, request).await.map_err(RequestError::Transport)
    }

    /// Retrieve the Context Source identity document.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn source_identity(&self, accept: Option<&str>) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().origin_join("/ngsi-ld/v1/info/sourceIdentity")?;
        let mut request = self.request(Method::GET, url.clone()); // clone: transport diagnostics retain the request URL.
        if let Some(accept) = accept {
            request = request.header("accept", accept);
        }
        self.send(Method::GET, url, request).await.map_err(RequestError::Transport)
    }

    async fn discovery_get(&self, path: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
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
