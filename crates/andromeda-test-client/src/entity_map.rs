//! Entity Map endpoint operations.

use crate::{response::TestResponse, test_client::TestClient, transport_error::RequestError};
use http::Method;
use urlencoding::encode;

impl TestClient {
    /// Retrieve an Entity Map.
    pub async fn retrieve_entity_map(&self, id: &str) -> Result<TestResponse, RequestError> {
        self.entity_map_request(Method::GET, id).await
    }

    /// Delete an Entity Map.
    pub async fn delete_entity_map(&self, id: &str) -> Result<TestResponse, RequestError> {
        self.entity_map_request(Method::DELETE, id).await
    }

    async fn entity_map_request(&self, method: Method, id: &str) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(&format!("entityMaps/{}", encode(id)))?;
        let request = self.request(method.clone(), url.clone()); // clone: the builder and transport diagnostics each own method and URL context.
        self.send(method, url, request).await.map_err(RequestError::Transport)
    }
}
