//! None of these is the client's doing: each is a property of a registration the broker admitted or of the broker's own configuration, so a broken one is answered `InternalError`.

use crate::forwarded_parameter::ForwardedParameter;
use andromeda_http_headers::{language::LanguageFilterError, received_protocol::ReceivedProtocolError};
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
};
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfoError;
use andromeda_ngsi_ld_query::geo::geo_query_error::GeoQueryError;
use andromeda_retry::schedule_error::BackoffScheduleError;
use mediatype::MediaTypeBuf;
use std::num::ParseIntError;
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
    /// An identifier a forwarded batch query selects by is not an NGSI-LD
    /// identifier, although the narrowing took it from a parsed query.
    #[error("an identifier of a forwarded batch query is not an NGSI-LD identifier: {0}")]
    ForwardedIdentifier(#[source] IdentifierParseError),
    /// The clause 4.10 members a forwarded query writes do not read back as a
    /// geo-query, although the narrowing wrote them from an admitted one.
    #[error("the geo-query members of a forwarded query do not read back as a geo-query: {0}")]
    ForwardedGeoQuery(#[source] GeoQueryError),
    /// A forwarded query writes some of `georel`, `geometry` and `coordinates`
    /// but not all three, which the narrowing writes together.
    #[error("a forwarded query writes only part of a geo-query")]
    PartialForwardedGeoQuery,
    /// The `lang` a forwarded query writes does not read back as a clause 4.15
    /// language filter, although the narrowing wrote it from a parsed one.
    #[error("the lang of a forwarded query does not read back as a language filter: {0}")]
    ForwardedLanguageFilter(#[source] LanguageFilterError),
    /// The `join` a forwarded query writes does not read back as a clause
    /// 4.5.23 retrieval, although the narrowing wrote it from one.
    #[error("the join of a forwarded query does not read back as a Linked Entity retrieval: {0}")]
    ForwardedJoin(#[source] strum::ParseError),
    /// The `joinLevel` a forwarded query writes does not read back as a
    /// positive depth, although the narrowing wrote it from one.
    #[error("the joinLevel of a forwarded query does not read back as a positive depth: {0}")]
    ForwardedJoinLevel(#[source] ParseIntError),
    /// A forwarded entity query writes a parameter the batch binding has no
    /// place for, which the narrowing of an entity query never writes.
    #[error("a forwarded batch query has no place for the {0} parameter")]
    BatchQueryParameter(ForwardedParameter),
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
    CooldownPolicy(#[source] BackoffScheduleError),
}
