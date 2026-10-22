//! The lifetime a Snapshot is created or patched with.

use crate::{error::SnapshotError, snapshot_invariant::SnapshotInvariant, snapshot_validation_reason::SnapshotValidationReason};
use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;
use andromeda_ngsi_ld_model::lifetime_expiry::expiry;
use chrono::{DateTime, Utc};
use std::time::Duration;

/// The lifetime a Snapshot starting at `starts_at` is given, capped at
/// `maximum_lifetime`, and the instant it expires.
pub(crate) fn effective_snapshot_lifetime(
    starts_at: DateTime<Utc>,
    lifetime: &Iso8601Duration,
    maximum_lifetime: Duration,
) -> Result<(Iso8601Duration, DateTime<Utc>), SnapshotError> {
    let expires_at = expiry(starts_at, lifetime).ok_or(SnapshotValidationReason::LifetimeOutOfRange)?;
    let maximum_expires_at = starts_at + chrono::Duration::from_std(maximum_lifetime).map_err(SnapshotInvariant::MaximumLifetime)?;
    if expires_at <= starts_at {
        Err(SnapshotValidationReason::LifetimeNotPositive.into())
    } else if expires_at > maximum_expires_at {
        Ok((Iso8601Duration::from_std(maximum_lifetime), maximum_expires_at))
    } else {
        Ok((*lifetime, expires_at))
    }
}
