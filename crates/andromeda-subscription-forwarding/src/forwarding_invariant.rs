//! The invariants the distributed subscription side relies on.

use andromeda_delivery::endpoint::error::EndpointParseError;
use andromeda_ngsi_ld_lexicon::identifier_length::IdentifierTooLong;
use andromeda_ngsi_ld_render::error::RenderError;
use thiserror::Error;

/// A distributed-subscription invariant that did not hold.
///
/// None of these is the client's doing, so a broken one is answered
/// `InternalError`.
#[derive(Debug, Error)]
pub enum ForwardingInvariant {
    /// A minted identifier's namespace-specific string is not a URN component.
    #[error("a minted URN is invalid: {0}")]
    MintedUrn(#[source] urn_rs::Error),
    /// A minted URN is past the identifier bound.
    #[error("a minted identifier is invalid: {0}")]
    MintedIdentifier(#[source] IdentifierTooLong),
    /// Merging a present entity with its other parts produced nothing.
    #[error("merging a present entity with its other parts produced nothing")]
    MergeProducedNothing,
    /// A re-assembled entity could not be rendered in the notification format.
    #[error("a re-assembled entity could not be rendered: {0}")]
    Rendering(#[source] RenderError),
    /// The local subscription's endpoint, admitted at creation, is no longer
    /// deliverable.
    #[error("the local subscription endpoint is not deliverable: {0}")]
    LocalEndpoint(#[source] EndpointParseError),
}
