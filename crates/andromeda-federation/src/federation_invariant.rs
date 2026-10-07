//! None of these is the client's doing: each is a property of a registration the broker admitted or of the broker's own configuration, so a broken one is answered `InternalError`.

use andromeda_http_headers::received_protocol::ReceivedProtocolError;
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
};
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfoError;
use http::header::{InvalidHeaderName, InvalidHeaderValue};
use iri_rs::InvalidIri;
use mediatype::MediaTypeBuf;
use thiserror::Error;
use url::Url;

/// A federation invariant that did not hold.
#[derive(Debug, Error)]
pub enum FederationInvariant {
    /// A registration's admitted `jsonldContext` is not an absolute IRI.
    #[error("a registration's jsonldContext is not an absolute IRI: {0}")]
    RegistrationJsonldContext(#[source] InvalidIri<String>),
    /// A registration's admitted `contentType` names a media type no forwarded
    /// request can be sent as, although clause 4.3.6.6 admits only
    /// `application/json` and `application/ld+json`.
    #[error("a registration's admitted contentType {media_type} is not a forwardable media type")]
    RegistrationContentType {
        /// The media type the registration named.
        media_type: MediaTypeBuf,
    },
    /// A geo-query geometry admitted under clause 4.10 cannot be written back
    /// to its wire form for a forwarded query.
    #[error("an admitted geo-query geometry cannot be written back to its wire form: {0}")]
    ForwardedGeometry(#[source] serde_json::Error),
    /// A name spelled for a forwarded request in its forwarding context is not
    /// an NGSI-LD name (clause 4.6.2): the context maps an IRI to text no name
    /// can be.
    #[error("a name spelled for a forwarded request is not an NGSI-LD name: {0}")]
    ForwardedName(#[source] NgsiLdReferenceError),
    /// An identifier a forwarded batch query selects by is not an NGSI-LD
    /// identifier, although the narrowing took it from a parsed query.
    #[error("an identifier of a forwarded batch query is not an NGSI-LD identifier: {0}")]
    ForwardedIdentifier(#[source] IdentifierParseError),
    /// The one attribute of a forwarded attribute fragment compacted to no
    /// attribute member, although the fragment names one.
    #[error("a forwarded attribute fragment compacted to no attribute")]
    FragmentWithoutAttribute,
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
