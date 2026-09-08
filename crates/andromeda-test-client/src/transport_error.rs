//! Typed endpoint, request, and response transport failures.

use crate::header::HeaderEncodingError;
use http::{Method, header::InvalidHeaderValue};
use thiserror::Error;
use url::{ParseError, Url};

/// Failure to construct a validated endpoint URL.
#[derive(Debug, Error)]
pub enum EndpointError {
    /// The supplied endpoint or relative path is not a valid URL.
    #[error("invalid broker endpoint URL: {0}")]
    InvalidUrl(#[source] ParseError),
}

/// Failure to validate a tenant name.
#[derive(Debug, Error)]
pub enum TenantNameError {
    /// The value cannot be encoded in an HTTP header.
    #[error("tenant name is not a valid HTTP header value: {0}")]
    InvalidHeaderValue(#[source] InvalidHeaderValue),
}

/// Failure while sending a test HTTP request or capturing its body.
#[derive(Debug, Error)]
#[error("{method} {url} failed: {source}")]
pub struct TransportError {
    /// HTTP method of the failed request.
    pub method: Method,
    /// Fully resolved URL of the failed request.
    pub url: Url,
    /// Underlying reqwest failure.
    #[source]
    pub source: reqwest::Error,
}

impl TransportError {
    /// Attach method and URL context to a reqwest failure.
    #[must_use]
    pub const fn new(method: Method, url: Url, source: reqwest::Error) -> TransportError {
        TransportError { method, url, source }
    }
}

/// Failure while constructing or sending an endpoint request.
#[derive(Debug, Error)]
pub enum RequestError {
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

/// Failure while constructing a test HTTP client with custom DNS mappings.
#[derive(Debug, Error)]
pub enum ClientBuildError {
    /// Reqwest rejected the client configuration.
    #[error("failed to build test HTTP client: {0}")]
    Reqwest(#[source] reqwest::Error),
}
