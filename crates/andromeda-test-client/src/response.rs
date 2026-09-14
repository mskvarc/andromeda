//! Byte-preserving HTTP response capture and explicit JSON decoding.

use bytes::Bytes;
use http::{
    HeaderMap,
    StatusCode,
    header::{LOCATION, ToStrError},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// HTTP response captured without assuming a body representation.
#[derive(Clone, Debug)]
pub struct TestResponse {
    /// Response status.
    pub status: StatusCode,
    /// Complete response headers, including multi-valued fields.
    pub headers: HeaderMap,
    /// Exact response body bytes after transport decoding.
    pub body: Bytes,
}

impl TestResponse {
    /// Construct a captured response from its HTTP parts.
    pub const fn new(status: StatusCode, headers: HeaderMap, body: Bytes) -> TestResponse {
        TestResponse { status, headers, body }
    }

    /// Decode the captured body as JSON without discarding the response context.
    ///
    /// # Errors
    ///
    /// Returns [`JsonBodyError::EmptyBody`] when the captured body has no bytes, and [`JsonBodyError::InvalidJson`] when those
    /// bytes are not valid JSON or do not describe a `T`.
    pub fn json<T>(&self) -> Result<T, JsonBodyError>
    where
        T: DeserializeOwned,
    {
        if self.body.is_empty() {
            return Err(JsonBodyError::EmptyBody { status: self.status });
        }
        serde_json::from_slice(&self.body).map_err(|source| JsonBodyError::InvalidJson {
            status: self.status,
            body: self.body.clone(), // clone: the decoding error retains the exact bytes while the captured response remains inspectable.
            source,
        })
    }

    /// Decode the captured body as an untyped JSON value.
    ///
    /// # Errors
    ///
    /// Returns [`JsonBodyError::EmptyBody`] when the captured body has no bytes, and [`JsonBodyError::InvalidJson`] when those
    /// bytes are not valid JSON.
    pub fn json_value(&self) -> Result<Value, JsonBodyError> {
        self.json()
    }

    /// Return the `Location` header when it is present and valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderDecodeError::InvalidHeaderValue`] when a `Location` header is present but is not valid visible text.
    pub fn location(&self) -> Result<Option<&str>, HeaderDecodeError> {
        self.headers
            .get(LOCATION)
            .map(|value| value.to_str().map_err(HeaderDecodeError::InvalidHeaderValue))
            .transpose()
    }
}

/// Failure to decode a captured response body as JSON.
#[derive(Debug, Error)]
pub enum JsonBodyError {
    /// The response had no body bytes.
    #[error("response with status {status} has an empty body")]
    EmptyBody {
        /// Status of the captured response.
        status: StatusCode,
    },
    /// The response body was present but was not valid JSON.
    #[error("response with status {status} contains invalid JSON: {source}")]
    InvalidJson {
        /// Status of the captured response.
        status: StatusCode,
        /// Original body bytes retained for diagnostics.
        body: Bytes,
        /// JSON decoder failure.
        #[source]
        source: serde_json::Error,
    },
}

/// Failure to decode a typed response header.
#[derive(Debug, Error)]
pub enum HeaderDecodeError {
    /// A present header did not contain valid visible text.
    #[error("response header contains invalid text: {0}")]
    InvalidHeaderValue(#[source] ToStrError),
}

#[cfg(test)]
mod tests {
    use crate::response::{JsonBodyError, TestResponse};
    use bytes::Bytes;
    use http::{HeaderMap, StatusCode};
    use pretty_assertions::assert_eq;
    use serde_json::Value;

    #[test]
    fn valid_json_decodes_to_the_requested_type() {
        let response = TestResponse::new(StatusCode::OK, HeaderMap::new(), Bytes::from_static(br#"{"answer":42}"#));
        let body: Value = response.json().unwrap_or_else(|error| panic!("static JSON is valid: {error}"));

        assert_eq!(body["answer"], 42);
    }

    #[test]
    fn empty_body_is_distinct_from_invalid_json() {
        let response = TestResponse::new(StatusCode::NO_CONTENT, HeaderMap::new(), Bytes::new());
        let error = response.json::<Value>().expect_err("empty bytes must not decode as JSON");

        assert!(matches!(
            error,
            JsonBodyError::EmptyBody {
                status: StatusCode::NO_CONTENT
            }
        ));
    }

    #[test]
    fn invalid_json_retains_status_and_original_bytes() {
        let response = TestResponse::new(StatusCode::BAD_GATEWAY, HeaderMap::new(), Bytes::from_static(b"not-json"));
        let error = response.json::<Value>().expect_err("invalid bytes must fail JSON decoding");

        match error {
            JsonBodyError::InvalidJson { status, body, source: _ } => {
                assert_eq!(status, StatusCode::BAD_GATEWAY);
                assert_eq!(body, Bytes::from_static(b"not-json"));
            }
            JsonBodyError::EmptyBody { status } => panic!("body was non-empty, status was {status}"),
        }
    }
}
