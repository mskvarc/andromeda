//! Typed HTTP header construction and inspection for test requests.

use andromeda_http_headers::link::header::Link;
use http::{HeaderMap, HeaderValue, header::ToStrError};
use iri_rs::IriRefBuf;
use std::str::FromStr;
use thiserror::Error;

/// Build the JSON-LD context `Link` header defined by NGSI-LD clause 6.3.5.
///
/// # Errors
///
/// Returns [`HeaderEncodingError::InvalidIri`] when `context_url` is not a valid IRI reference, and
/// [`HeaderEncodingError::InvalidLink`] when the typed `Link` cannot be rendered as an HTTP header value.
pub fn jsonld_link_header(context_url: &str) -> Result<HeaderValue, HeaderEncodingError> {
    let context_url = IriRefBuf::from_str(context_url).map_err(|_error| HeaderEncodingError::InvalidIri)?;
    Link::jsonld_context(context_url).to_header_value().ok_or(HeaderEncodingError::InvalidLink)
}

/// Collect every valid `NGSILD-Warning` response header value.
///
/// # Errors
///
/// Returns [`HeaderEncodingError::InvalidText`] when any `NGSILD-Warning` header value is not valid visible text.
pub fn warning_header_values(headers: &HeaderMap) -> Result<Vec<&str>, HeaderEncodingError> {
    headers
        .get_all("ngsild-warning")
        .iter()
        .map(|value| value.to_str().map_err(HeaderEncodingError::InvalidText))
        .collect()
}

/// Failure to encode or decode a reusable test header.
#[derive(Debug, Error)]
pub enum HeaderEncodingError {
    /// The context is not a valid IRI reference.
    #[error("JSON-LD context is not a valid IRI reference")]
    InvalidIri,
    /// A typed Link value could not be represented as an HTTP header.
    #[error("JSON-LD context Link cannot be encoded as an HTTP header")]
    InvalidLink,
    /// A response header contained invalid text.
    #[error("response header contains invalid text: {0}")]
    InvalidText(#[source] ToStrError),
}
