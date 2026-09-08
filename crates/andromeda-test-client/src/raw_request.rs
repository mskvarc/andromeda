//! Low-level byte-oriented requests for malformed and negative test cases.

use crate::{
    response::TestResponse,
    test_client::TestClient,
    transport_error::{EndpointError, TransportError},
};
use http::{HeaderValue, Method, header::InvalidHeaderValue};
use reqwest::RequestBuilder;
use serde_json::Value;
use std::time::Duration;
use url::Url;

/// Raw request construction failure.
#[derive(Debug, thiserror::Error)]
pub enum RawRequestError {
    /// The supplied root-relative path could not be resolved.
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
    /// A raw header value was not valid HTTP header syntax.
    #[error("invalid raw request header value: {0}")]
    Header(#[from] InvalidHeaderValue),
    /// Sending or capturing the request failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
}

/// Raw request builder retaining method and URL context.
pub struct RawRequest<'a> {
    client: &'a TestClient,
    prepared: Result<PreparedRequest, EndpointError>,
    header_error: Option<InvalidHeaderValue>,
}

struct PreparedRequest {
    method: Method,
    url: Url,
    request: RequestBuilder,
}

impl<'a> RawRequest<'a> {
    /// Attach one validated HTTP header.
    #[must_use]
    pub fn header(mut self, name: &str, value: impl RawHeaderValue) -> RawRequest<'a> {
        if let Ok(prepared) = self.prepared.as_mut() {
            match value.into_header_value() {
                Ok(value) => {
                    if let Some(request) = prepared.request.try_clone() {
                        prepared.request = request.header(name, value);
                    }
                }
                Err(error) => self.header_error = Some(error),
            }
        }
        self
    }

    /// Serialize and attach a JSON body.
    #[must_use]
    pub fn json(mut self, body: &Value) -> RawRequest<'a> {
        if let Ok(prepared) = self.prepared.as_mut()
            && let Some(request) = prepared.request.try_clone()
        {
            prepared.request = request.json(body);
        }
        self
    }

    /// Attach exact body bytes.
    #[must_use]
    pub fn body(mut self, body: Vec<u8>) -> RawRequest<'a> {
        if let Ok(prepared) = self.prepared.as_mut()
            && let Some(request) = prepared.request.try_clone()
        {
            prepared.request = request.body(body);
        }
        self
    }

    /// Set an explicit request timeout.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> RawRequest<'a> {
        if let Ok(prepared) = self.prepared.as_mut()
            && let Some(request) = prepared.request.try_clone()
        {
            prepared.request = request.timeout(timeout);
        }
        self
    }

    /// Send the request and preserve the response body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`RawRequestError::Header`] when a value passed to [`RawRequest::header`] was not valid HTTP header syntax,
    /// [`RawRequestError::Endpoint`] when the request path could not be resolved against the broker origin, and
    /// [`RawRequestError::Transport`] when the request cannot be sent or its response body cannot be captured.
    pub async fn send(self) -> Result<TestResponse, RawRequestError> {
        if let Some(error) = self.header_error {
            return Err(RawRequestError::Header(error));
        }
        let prepared = self.prepared?;
        self.client
            .send(prepared.method, prepared.url, prepared.request)
            .await
            .map_err(RawRequestError::Transport)
    }
}

/// Values accepted by the raw request header boundary.
pub trait RawHeaderValue {
    /// Convert the caller's value to an HTTP header value.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHeaderValue`] when the value contains bytes that an HTTP header value may not carry.
    fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue>;
}

impl RawHeaderValue for &str {
    fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue> {
        HeaderValue::from_str(self)
    }
}

impl RawHeaderValue for &&str {
    fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue> {
        HeaderValue::from_str(self)
    }
}

impl RawHeaderValue for &String {
    fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue> {
        HeaderValue::from_str(self)
    }
}

impl RawHeaderValue for HeaderValue {
    fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue> {
        Ok(self)
    }
}

impl RawHeaderValue for &HeaderValue {
    fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue> {
        // Header values are immutable, so cloning only increments owned storage.
        Ok(self.clone())
    }
}

impl TestClient {
    /// Build a raw request against a broker-origin path such as `/ngsi-ld/v1/entities`.
    pub fn raw(&self, method: Method, path: &str) -> RawRequest<'_> {
        let prepared = self.endpoint().origin_join(path).map(|url| {
            let request = self.request(method.clone(), url.clone()); // clone: the prepared request and its builder each own method and URL context.
            PreparedRequest { method, url, request }
        });
        RawRequest {
            client: self,
            prepared,
            header_error: None,
        }
    }
}
