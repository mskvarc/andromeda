//! Batch endpoint operations.

use crate::{header::jsonld_link_header, response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::APPLICATION_LD_JSON;
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;

impl TestClient {
    /// Create a batch of entities.
    pub async fn batch_create(&self, body: &Value) -> Result<TestResponse, RequestError> {
        self.send_batch("entityOperations/create", Method::POST, body, None, None).await
    }

    /// Upsert a batch of entities.
    pub async fn batch_upsert(&self, body: &Value) -> Result<TestResponse, RequestError> {
        self.send_batch("entityOperations/upsert", Method::POST, body, None, None).await
    }

    /// Update a batch of entities.
    pub async fn batch_update(&self, body: &Value) -> Result<TestResponse, RequestError> {
        self.send_batch("entityOperations/update", Method::POST, body, None, None).await
    }

    /// Merge a batch of entities.
    pub async fn batch_merge(&self, body: &Value) -> Result<TestResponse, RequestError> {
        self.send_batch("entityOperations/merge", Method::POST, body, None, None).await
    }

    /// Delete a batch of entities.
    pub async fn batch_delete(&self, body: &Value) -> Result<TestResponse, RequestError> {
        self.send_batch("entityOperations/delete", Method::POST, body, None, None).await
    }

    /// Query a batch of entities.
    pub async fn batch_query(&self, body: &Value, accept: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.send_batch("entityOperations/query", Method::POST, body, Some(accept), context).await
    }

    async fn send_batch(&self, path: &str, method: Method, body: &Value, accept: Option<&str>, context: Option<&str>) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(path)?;
        let mut request = self
            .request(method.clone(), url.clone()) // clone: the builder and transport diagnostics each own method and URL context.
            .header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string())
            .json(body);
        if let Some(accept) = accept {
            request = request.header("accept", accept);
        }
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(method, url, request).await.map_err(RequestError::Transport)
    }
}
