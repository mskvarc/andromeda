//! The invariants subscription policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::identifier_length::IdentifierTooLong;
use thiserror::Error;

/// A subscription policy invariant that did not hold.
///
/// None of these is the client's doing, so a broken one is answered
/// `InternalError`.
#[derive(Debug, Error)]
pub enum SubscriptionInvariant {
    /// The distributed side of a subscription (its forwarded companions at the
    /// context sources) could not complete the operation.
    #[error("the distributed side of the subscription could not complete the operation")]
    DistributedSide,
    /// A freshly minted subscription identifier is not an identifier.
    #[error("a minted subscription identifier is invalid: {0}")]
    MintedIdentifier(#[source] IdentifierTooLong),
    /// An attribute name the dictionary was asked to intern came back without a
    /// key.
    #[error("an attribute name was not interned by the name dictionary")]
    AttributeNotInterned,
}
