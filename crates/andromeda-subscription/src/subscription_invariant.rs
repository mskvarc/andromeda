//! The invariants subscription policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::SubscriptionIdParseError;
use andromeda_ngsi_ld_model::{subscription::unreadable_member::UnreadableMember, written_subscription_terms::WrittenTermsError};
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
    MintedIdentifier(#[source] SubscriptionIdParseError),
    /// An attribute name the dictionary was asked to intern came back without a
    /// key.
    #[error("an attribute name was not interned by the name dictionary")]
    AttributeNotInterned,
    /// A written `q` reached term expansion without the admission of the
    /// request that wrote it, or an admission reached it with no `q` to
    /// compile.
    #[error("the q of a subscription request was not admitted exactly once before it was compiled")]
    QNotAdmitted,
    /// A stored name has no written spelling, though the store checks every
    /// name against the bound a written name keeps.
    #[error("a stored subscription holds a name no written name can spell: {0}")]
    StoredNameNotWritable(#[from] WrittenTermsError),
    /// A stored member the broker cannot read is not replaced by the update, so writing the subscription back would drop it and widen what it selects.
    #[error("a stored subscription member the broker cannot read is not replaced by the update: {0}")]
    UnreadableMemberKept(UnreadableMember),
}
