//! `/entities` endpoint operations.

use crate::{
    header::{HeaderEncodingError, jsonld_link_header},
    response::{HeaderDecodeError, TestResponse},
    test_client::TestClient,
    transport_error::{EndpointError, TransportError},
};
use andromeda_http_headers::media_type::{APPLICATION_JSON, APPLICATION_LD_JSON, ResponseMediaType};
use http::{Method, header::CONTENT_TYPE};
use serde_json::Value;
use thiserror::Error;
use urlencoding::encode;

/// Entity endpoint request failure.
#[derive(Debug, Error)]
pub enum EntityRequestError {
    /// Endpoint URL construction failed.
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
    /// A typed header could not be encoded.
    #[error(transparent)]
    Header(#[from] HeaderEncodingError),
    /// Sending or capturing the response failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
}

/// Request builder for creating one entity.
pub struct CreateEntity<'a> {
    client: &'a TestClient,
    body: &'a Value,
    representation: EntityRepresentation<'a>,
}

enum EntityRepresentation<'a> {
    JsonLd,
    Json { context: &'a str },
}

impl<'a> CreateEntity<'a> {
    /// Send `application/ld+json` with the context retained in the body.
    #[must_use]
    pub const fn with_ld_json(mut self) -> CreateEntity<'a> {
        self.representation = EntityRepresentation::JsonLd;
        self
    }

    /// Send `application/json` with the supplied JSON-LD context Link header.
    #[must_use]
    pub const fn with_json(mut self, context: &'a str) -> CreateEntity<'a> {
        self.representation = EntityRepresentation::Json { context };
        self
    }

    /// Send the create request.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when `entities` cannot be resolved against the broker endpoint,
    /// [`EntityRequestError::Header`] when an `application/json` request's context is not a valid IRI reference or cannot be
    /// encoded as a `Link` header, and [`EntityRequestError::Transport`] when the request cannot be sent or its response body
    /// cannot be captured.
    pub async fn send(self) -> Result<TestResponse, EntityRequestError> {
        let url = self.client.endpoint().join("entities")?;
        let mut body = self.body;
        let request = match self.representation {
            EntityRepresentation::JsonLd => self
                .client
                .request(Method::POST, url.clone()) // clone: transport diagnostics retain the same URL after the request builder consumes its copy.
                .header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string())
                .json(body),
            EntityRepresentation::Json { context } => {
                let mut stripped = self.body.clone(); // clone: JSON requests remove @context without mutating the caller's fixture.
                if let Some(object) = stripped.as_object_mut() {
                    object.remove("@context");
                }
                body = &stripped;
                let request = self
                    .client
                    .request(Method::POST, url.clone()) // clone: transport diagnostics retain the same URL after the request builder consumes its copy.
                    .header(CONTENT_TYPE, APPLICATION_JSON.to_string())
                    .header("link", jsonld_link_header(context)?);
                return self
                    .client
                    .send(Method::POST, url, request.json(body))
                    .await
                    .map_err(EntityRequestError::Transport);
            }
        };
        self.client.send(Method::POST, url, request).await.map_err(EntityRequestError::Transport)
    }
}

impl TestClient {
    /// Begin a create-entity request.
    pub const fn post_entity<'a>(&'a self, body: &'a Value) -> CreateEntity<'a> {
        CreateEntity {
            client: self,
            body,
            representation: EntityRepresentation::JsonLd,
        }
    }

    /// Retrieve one entity.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`EntityRequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`EntityRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn get_entity(&self, id: &str, query: &str, accept: &str, context: Option<&str>) -> Result<TestResponse, EntityRequestError> {
        let suffix = query_suffix(query);
        let url = self.endpoint().join(&format!("entities/{}{suffix}", encode(id)))?;
        let mut request = self.request(Method::GET, url.clone()).header("accept", accept); // clone: transport diagnostics retain the request URL.
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::GET, url, request).await.map_err(EntityRequestError::Transport)
    }

    /// Query entities.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`EntityRequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`EntityRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn query_entities(&self, query: &str, accept: &str, context: Option<&str>) -> Result<TestResponse, EntityRequestError> {
        let url = self.endpoint().join(&format!("entities{}", query_suffix(query)))?;
        let mut request = self.request(Method::GET, url.clone()).header("accept", accept); // clone: transport diagnostics retain the request URL.
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::GET, url, request).await.map_err(EntityRequestError::Transport)
    }

    /// Delete one entity.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when the path cannot be resolved against the broker endpoint, and
    /// [`EntityRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn delete_entity(&self, id: &str) -> Result<TestResponse, EntityRequestError> {
        let url = self.endpoint().join(&format!("entities/{}", encode(id)))?;
        let request = self.request(Method::DELETE, url.clone()); // clone: transport diagnostics retain the request URL.
        self.send(Method::DELETE, url, request).await.map_err(EntityRequestError::Transport)
    }

    /// Delete entities selected by a query.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`EntityRequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`EntityRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn delete_entities_by_query(&self, query: &str, context: Option<&str>) -> Result<TestResponse, EntityRequestError> {
        let url = self.endpoint().join(&format!("entities{}", query_suffix(query)))?;
        let mut request = self.request(Method::DELETE, url.clone()); // clone: transport diagnostics retain the request URL.
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::DELETE, url, request).await.map_err(EntityRequestError::Transport)
    }

    /// Merge an entity fragment.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`EntityRequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`EntityRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn merge_entity(&self, id: &str, fragment: &Value, context: Option<&str>) -> Result<TestResponse, EntityRequestError> {
        let url = self.endpoint().join(&format!("entities/{}", encode(id)))?;
        let mut body = fragment.clone(); // clone: context stripping must not mutate the caller's fragment.
        let mut request = self.request(Method::PATCH, url.clone()); // clone: transport diagnostics retain the request URL.
        match context {
            Some(context) => {
                if let Some(object) = body.as_object_mut() {
                    object.remove("@context");
                }
                request = request
                    .header(CONTENT_TYPE, APPLICATION_JSON.to_string())
                    .header("link", jsonld_link_header(context)?);
            }
            None => request = request.header(CONTENT_TYPE, APPLICATION_LD_JSON.to_string()),
        }
        self.send(Method::PATCH, url, request.json(&body)).await.map_err(EntityRequestError::Transport)
    }

    /// Replace an entity representation.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRequestError::Endpoint`] when the path cannot be resolved against the broker endpoint,
    /// [`EntityRequestError::Header`] when `context` is not a valid IRI reference or cannot be encoded as a `Link` header, and
    /// [`EntityRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn replace_entity(&self, id: &str, entity: &Value, content_type: &str, context: Option<&str>) -> Result<TestResponse, EntityRequestError> {
        let url = self.endpoint().join(&format!("entities/{}", encode(id)))?;
        let mut body = entity.clone(); // clone: content-type normalization must not mutate the caller's entity.
        let mut request = self
            .request(Method::PUT, url.clone()) // clone: transport diagnostics retain the request URL.
            .header(CONTENT_TYPE, content_type)
            .header("accept", ResponseMediaType::JsonLd.to_string());
        if content_type == "application/json"
            && let Some(object) = body.as_object_mut()
        {
            object.remove("@context");
        }
        if let Some(context) = context {
            request = request.header("link", jsonld_link_header(context)?);
        }
        self.send(Method::PUT, url, request.json(&body)).await.map_err(EntityRequestError::Transport)
    }
}

/// Decode `NGSILD-Results-Count` when it is present and numeric.
#[must_use]
pub fn results_count(headers: &http::HeaderMap) -> Option<usize> {
    headers
        .get("ngsild-results-count")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

/// Return a required create response location.
///
/// # Errors
///
/// Returns [`RequiredLocationError::UnexpectedStatus`] when the response status is not `201 Created`,
/// [`RequiredLocationError::Header`] when a `Location` header is present but is not valid visible text, and
/// [`RequiredLocationError::Missing`] when the response carries no `Location` header.
pub fn require_location(response: &TestResponse) -> Result<&str, RequiredLocationError> {
    if response.status != http::StatusCode::CREATED {
        return Err(RequiredLocationError::UnexpectedStatus(response.status));
    }
    response
        .location()
        .map_err(RequiredLocationError::Header)?
        .ok_or(RequiredLocationError::Missing)
}

/// Failure to obtain the location of a created resource.
#[derive(Debug, Error)]
pub enum RequiredLocationError {
    /// The response was not `201 Created`.
    #[error("expected 201 Created, got {0}")]
    UnexpectedStatus(http::StatusCode),
    /// The response omitted `Location`.
    #[error("201 Created response omitted Location")]
    Missing,
    /// The location header was malformed.
    #[error(transparent)]
    Header(#[from] HeaderDecodeError),
}

fn query_suffix(query: &str) -> String {
    if query.is_empty() { String::new() } else { format!("?{query}") }
}
