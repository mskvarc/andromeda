//! None of these is the client's doing: each is a property of a registration the broker admitted or of the broker's own configuration, so a broken one is answered `InternalError`.

use crate::config::FederationConfigError;
use andromeda_http_headers::received_protocol::ReceivedProtocolError;
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
};
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfoError;
use mediatype::MediaTypeBuf;
use thiserror::Error;
use url::Url;

/// A federation invariant that did not hold.
#[derive(Debug, Error)]
pub enum FederationInvariant {
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
    /// A core term a split query's filters default to (`location`, `scope`)
    /// is not an attribute name, although clause 4.6.2 makes every core term
    /// one.
    #[error("a core term is not an attribute name: {0}")]
    CoreAttributeName(#[source] NgsiLdReferenceError),
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
    /// The cooldown policy does not hold, although configuration loading
    /// validates it before any forwarding service is built.
    #[error("the context source cooldown policy is invalid: {0}")]
    CooldownPolicy(#[source] FederationConfigError),
}
