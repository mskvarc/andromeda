//! Subscription endpoint operations.

use crate::{header::jsonld_link_header, response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::{APPLICATION_JSON, APPLICATION_LD_JSON, ResponseMediaType};
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;
use urlencoding::encode;

impl TestClient {
    /// Create a subscription.
    pub async fn create_subscription(&self, body: &Value) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join("subscriptions")?;
        let request = self
            .request(Method::POST, url.clone()) // clone: transport diagnostics retain the request URL.
            .header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string())
            .json(body);
        self.send(Method::POST, url, request).await.map_err(RequestError::Transport)
    }

    /// Retrieve one subscription.
    pub async fn retrieve_subscription(&self, id: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.subscription_get(&format!("subscriptions/{}", encode(id)), context).await
    }

    /// Query subscriptions.
    pub async fn query_subscriptions(&self, query: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
        self.subscription_get(&format!("subscriptions{}", query_suffix(query)), context).await
    }

    /// Update one subscription.
    pub async fn update_subscription(&self, id: &str, fragment: &Value, context: Option<&str>) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("subscriptions/{}", encode(id)))?;
        let mut request = self
            .request(Method::PATCH, url.clone()) // clone: transport diagnostics retain the request URL.
            .header(CONTENT_TYPE, APPLICATION_JSON.to_string())
            .json(fragment);
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::PATCH, url, request).await.map_err(RequestError::Transport)
    }

    /// Delete one subscription.
    pub async fn delete_subscription(&self, id: &str) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("subscriptions/{}", encode(id)))?;
        let request = self.request(Method::DELETE, url.clone()); // clone: transport diagnostics retain the request URL.
        self.send(Method::DELETE, url, request).await.map_err(RequestError::Transport)
    }

    async fn subscription_get(&self, path: &str, context: Option<&str>) -> Result<TestResponse, RequestError> {
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
