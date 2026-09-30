//! The invariants federation relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a property of a registration the
//! broker admitted or of the broker's own configuration, so a broken one is
//! answered `InternalError`.

use andromeda_http_headers::received_protocol::ReceivedProtocolError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfoError;
use http::header::{InvalidHeaderName, InvalidHeaderValue};
use iri_rs::InvalidIri;
use thiserror::Error;
use url::Url;

/// A federation invariant that did not hold.
#[derive(Debug, Error)]
pub enum FederationInvariant {
    /// A registration's admitted `jsonldContext` is not an IRI reference.
    #[error("a registration's jsonldContext is not an IRI reference: {0}")]
    RegistrationJsonldContext(#[source] InvalidIri<String>),
    /// A registration's admitted `contextSourceInfo` no longer reads as the
    /// clause 4.3.6.6 pairs it was admitted as.
    #[error("registration {registration} carries an invalid contextSourceInfo: {source}")]
    ContextSourceInfo {
        /// The registration whose pairs failed.
        registration: Identifier,
        /// Why the pairs failed.
        #[source]
        source: ContextSourceInfoError,
    },
    /// A `contextSourceInfo` key is not an HTTP header name.
    #[error("a contextSourceInfo key is not an HTTP header name: {0}")]
    ContextSourceInfoHeaderName(#[source] InvalidHeaderName),
    /// A `contextSourceInfo` value is not an HTTP header value.
    #[error("a contextSourceInfo value is not an HTTP header value: {0}")]
    ContextSourceInfoHeaderValue(#[source] InvalidHeaderValue),
    /// A registration's endpoint cannot carry a path.
    #[error("the context source endpoint {endpoint} cannot be a base URL")]
    EndpointCannotBeBase {
        /// The registered endpoint.
        endpoint: Box<Url>,
    },
    /// The HTTP client used for forwarding could not be built.
    #[error("the HTTP client for context source forwarding could not be built: {0}")]
    HttpClient(#[source] reqwest::Error),
    /// The protocol the broker forwards over is not a `Via` protocol token.
    #[error("the forwarding protocol is not a Via protocol: {0}")]
    ForwardingProtocol(#[source] ReceivedProtocolError),
}
