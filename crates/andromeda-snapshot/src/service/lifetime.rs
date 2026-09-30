//! The lifetime a Snapshot is created or patched with.

use crate::{error::SnapshotError, snapshot_invariant::SnapshotInvariant, snapshot_validation_reason::SnapshotValidationReason};
use andromeda_ngsi_ld_model::lifetime_expiry::expiry;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
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
        Ok((iso8601_lifetime(maximum_lifetime)?, maximum_expires_at))
    } else {
        Ok((*lifetime, expires_at))
    }
}

/// A configured lifetime as the ISO 8601 duration a Snapshot records.
pub(crate) fn iso8601_lifetime(duration: Duration) -> Result<Iso8601Duration, SnapshotError> {
    let total_seconds = duration.as_secs();
    let days = total_seconds / 86_400;
    let after_days = total_seconds % 86_400;
    let hours = after_days / 3_600;
    let after_hours = after_days % 3_600;
    let minutes = after_hours / 60;
    let seconds = after_hours % 60;
    let value = if days > 0 {
        format!("P{days}DT{hours}H{minutes}M{seconds}S")
    } else {
        format!("PT{hours}H{minutes}M{seconds}S")
    };
    value.parse().map_err(|_unparsable| SnapshotInvariant::DefaultLifetime.into())
}
