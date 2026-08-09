//! Reconstruction of the broker origin from an incoming `Host` header.

use crate::error::AndromedaError;
use axum::http::{HeaderMap, header::HOST};
use url::Url;

/// Parse the request's `Host` header into the origin used for broker-hosted resources.
pub fn request_origin(headers: &HeaderMap) -> Result<Option<Url>, AndromedaError> {
    headers
        .get(HOST)
        .map(|value| {
            let host = value
                .to_str()
                .map_err(|error| AndromedaError::malformed_request(format!("Host header is not valid text: {error}")))?;
            Url::parse(&format!("http://{host}"))
                .map_err(|error| AndromedaError::malformed_request(format!("Host header does not identify a valid origin: {error}")))
        })
        .transpose()
}
