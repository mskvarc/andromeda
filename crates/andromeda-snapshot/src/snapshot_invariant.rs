//! The invariants Snapshot policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::SnapshotIdParseError};
use chrono::OutOfRangeError;
use std::num::TryFromIntError;
use thiserror::Error;

/// A Snapshot policy invariant that did not hold.
///
/// None of these is the client's doing, so a broken one is answered
/// `InternalError`.
#[derive(Debug, Error)]
pub enum SnapshotInvariant {
    /// The request context a Snapshot runs its queries under could not be
    /// materialised.
    #[error("the Snapshot request context could not be materialized")]
    RequestContextUnmaterialized,
    /// A freshly minted Snapshot identifier is not an identifier.
    #[error("a minted Snapshot identifier is invalid: {0}")]
    MintedIdentifier(#[source] SnapshotIdParseError),
    /// The configured maximum lifetime cannot be expressed as a calendar duration.
    #[error("the configured maximum Snapshot lifetime is out of range: {0}")]
    MaximumLifetime(#[source] OutOfRangeError),
    /// The configured default lifetime does not read as an ISO 8601 duration.
    #[error("the configured default Snapshot lifetime is not an ISO 8601 duration")]
    DefaultLifetime,
    /// A Snapshot that is being prepared owns no durable staging branch.
    #[error("Snapshot {snapshot} has no durable staging context target")]
    NoStagingTarget {
        /// The Snapshot being prepared.
        snapshot: Identifier,
    },
    /// The configured preparation lease cannot be expressed as a calendar duration.
    #[error("the configured Snapshot preparation lease is out of range: {0}")]
    LeaseOutOfRange(#[source] OutOfRangeError),
    /// A lease deadline exceeds the supported timestamp range.
    #[error("a Snapshot lease deadline exceeds the supported timestamp range")]
    LeaseDeadlineOverflow,
    /// The Snapshot a clone reads from no longer exists.
    #[error("clone source {source_snapshot} does not exist")]
    CloneSourceMissing {
        /// The Snapshot the clone was to read from.
        source_snapshot: Identifier,
    },
    /// The Snapshot is no longer preparing under the claim that tried to write it.
    #[error("Snapshot {snapshot} is no longer preparing under this claim")]
    ClaimLost {
        /// The Snapshot whose claim was lost.
        snapshot: Identifier,
    },
    /// The preparation row changed while its promotion was being applied.
    #[error("the Snapshot preparation row changed under the promotion")]
    PreparationChangedUnderPromotion,
    /// A tenant quota was exceeded with no Snapshot left to evict.
    #[error("the Snapshot quota was exceeded without an evictable Snapshot")]
    QuotaWithoutEvictable,
    /// The configured status-notification error bound is unusable.
    #[error("the configured Snapshot notification error bound is unusable")]
    NotificationErrorBound,
    /// The configured status-notification backoff does not fit its counter.
    #[error("the Snapshot notification backoff is out of range: {0}")]
    NotificationBackoff(#[source] TryFromIntError),
}
