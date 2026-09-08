//! Reconstruction of the broker origin from an incoming `Host` header.

use axum::http::HeaderMap;
use headers::HeaderMapExt;
use thiserror::Error;
use url::Url;

/// A malformed `Host` header that cannot identify the broker origin.
#[derive(Debug, Error)]
pub enum RequestOriginError {
    /// The header bytes are not valid visible text.
    #[error("Host header is not valid text: {detail}")]
    InvalidText {
        /// Header decoding detail.
        detail: String,
    },
    /// The header text cannot form an HTTP origin.
    #[error("Host header does not identify a valid origin: {detail}")]
    InvalidOrigin {
        /// URL parsing detail.
        detail: String,
    },
}

/// Parse the request's `Host` header into the origin used for broker-hosted resources.
///
/// # Errors
///
/// Returns [`RequestOriginError::InvalidOrigin`] when a `Host` header is
/// present but its authority does not form a valid `http` URL. A request
/// without a `Host` header yields `Ok(None)`.
pub fn request_origin(headers: &HeaderMap) -> Result<Option<Url>, RequestOriginError> {
    headers
        .typed_get::<headers::Host>()
        .map(|host| Url::parse(&format!("http://{host}")).map_err(|error| RequestOriginError::InvalidOrigin { detail: error.to_string() }))
        .transpose()
}
