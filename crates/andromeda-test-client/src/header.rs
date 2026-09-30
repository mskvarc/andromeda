//! Typed HTTP header construction and inspection for test requests.

use andromeda_http_headers::{
    link::{header::Link, relation_type::RelationType, value::LinkValue},
    media_type::APPLICATION_LD_JSON,
};
use http::{HeaderMap, HeaderValue, header::ToStrError};
use iri_rs::IriRefBuf;
use mediatype::MediaTypeBuf;
use std::str::FromStr;
use thiserror::Error;

/// Build the JSON-LD context `Link` header defined by NGSI-LD clause 6.3.5.
///
/// A request may name its context by a relative reference, which the broker
/// resolves against the request's own URI (IETF RFC 8288 section 3.1), so the
/// target is taken as an IRI reference rather than an absolute IRI.
///
/// # Errors
///
/// Returns [`HeaderEncodingError::InvalidIri`] when `context_url` is not a valid IRI reference, and
/// [`HeaderEncodingError::InvalidLink`] when the typed `Link` cannot be rendered as an HTTP header value.
pub fn jsonld_link_header(context_url: &str) -> Result<HeaderValue, HeaderEncodingError> {
    let context_url = IriRefBuf::from_str(context_url).map_err(|_error| HeaderEncodingError::InvalidIri)?;
    let context_link = LinkValue::new(context_url)
        .push_rel(RelationType::JsonLdContext)
        .set_media_type(MediaTypeBuf::from(&APPLICATION_LD_JSON));
    Link::new(vec![context_link]).to_header_value().ok_or(HeaderEncodingError::InvalidLink)
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
