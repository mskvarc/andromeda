//! The invariants the distributed subscription side relies on.

use andromeda_delivery::endpoint::error::EndpointParseError;
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::{NotificationIdParseError, SubscriptionIdParseError};
use andromeda_ngsi_ld_render::error::RenderError;
use thiserror::Error;

/// A distributed-subscription invariant that did not hold.
///
/// None of these is the client's doing, so a broken one is answered
/// `InternalError`.
#[derive(Debug, Error)]
pub enum ForwardingInvariant {
    /// A subscription identifier minted for a peer is not one.
    #[error("a minted subscription identifier is invalid: {0}")]
    MintedSubscriptionId(#[source] SubscriptionIdParseError),
    /// A notification identifier minted for the local subscriber is not one.
    #[error("a minted notification identifier is invalid: {0}")]
    MintedNotificationId(#[source] NotificationIdParseError),
    /// Merging a present entity with its other parts produced nothing.
    #[error("merging a present entity with its other parts produced nothing")]
    MergeProducedNothing,
    /// A re-assembled entity, every part of it already read under the local
    /// subscription's context, could not be expanded under it again for the
    /// local filters.
    #[error("a re-assembled entity could not be expanded under the local context: {0}")]
    LocalExpansion(#[source] ExpansionError),
    /// A re-assembled entity could not be rendered in the notification format.
    #[error("a re-assembled entity could not be rendered: {0}")]
    Rendering(#[source] RenderError),
    /// The local subscription's endpoint, admitted at creation, is no longer
    /// deliverable.
    #[error("the local subscription endpoint is not deliverable: {0}")]
    LocalEndpoint(#[source] EndpointParseError),
}
