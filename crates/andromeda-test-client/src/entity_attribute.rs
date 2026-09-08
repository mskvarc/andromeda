//! Entity Attribute endpoint operations.

use crate::{header::jsonld_link_header, response::TestResponse, test_client::TestClient, transport_error::RequestError};
use andromeda_http_headers::media_type::APPLICATION_LD_JSON;
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;
use urlencoding::encode;

/// Selection of all dataset instances or one default/specific instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatasetSelection {
    /// Delete all dataset instances of the attribute.
    All,
    /// Delete the default instance.
    Default,
    /// Delete the instance identified by the supplied dataset IRI.
    Dataset,
}

impl TestClient {
    /// Append attributes to an entity.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn append_entity_attrs(&self, id: &str, fragment: &Value) -> Result<TestResponse, RequestError> {
        self.send_attribute_fragment(Method::POST, &format!("entities/{}/attrs", encode(id)), fragment)
            .await
    }

    /// Update a set of entity attributes.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn update_entity_attrs(&self, id: &str, fragment: &Value) -> Result<TestResponse, RequestError> {
        self.send_attribute_fragment(Method::PATCH, &format!("entities/{}/attrs", encode(id)), fragment)
            .await
    }

    /// Partially update one entity attribute.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn partial_update_attr(&self, id: &str, attribute: &str, fragment: &Value) -> Result<TestResponse, RequestError> {
        self.send_attribute_fragment(Method::PATCH, &format!("entities/{}/attrs/{}", encode(id), encode(attribute)), fragment)
            .await
    }

    /// Delete an entity attribute instance or all its dataset instances.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`RequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`RequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn delete_entity_attr(
        &self,
        id: &str,
        attribute: &str,
        dataset_id: Option<&str>,
        selection: DatasetSelection,
        context: Option<&str>,
    ) -> Result<TestResponse, RequestError> {
        let mut query = Vec::new();
        if let Some(dataset_id) = dataset_id {
            query.push(format!("datasetId={}", encode(dataset_id)));
        }
        if selection == DatasetSelection::All {
            query.push("deleteAll=true".to_owned());
        }
        let suffix = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };
        let url = self.endpoint().join(&format!("entities/{}/attrs/{}{suffix}", encode(id), encode(attribute)))?;
        let mut request = self.request(Method::DELETE, url.clone()); // clone: transport diagnostics retain the request URL.
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::DELETE, url, request).await.map_err(RequestError::Transport)
    }

    async fn send_attribute_fragment(&self, method: Method, path: &str, fragment: &Value) -> Result<TestResponse, RequestError> {
        let url = self.endpoint().join(path)?;
        let request = self
            .request(method.clone(), url.clone()) // clone: the builder and transport diagnostics each own method and URL context.
            .header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string())
            .json(fragment);
        self.send(method, url, request).await.map_err(RequestError::Transport)
    }
}
