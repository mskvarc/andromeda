//! Durable Snapshot surrogate keys, which name a Snapshot's branch schemas.

use thiserror::Error;

/// Positive database-issued surrogate of one Snapshot control record.
///
/// The key names the Snapshot's PostgreSQL branch schemas: `snap_<key>` for the
/// published branch scoping resolves and `snap_<key>_stg` for the private
/// preparation branch capture and clone workers write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SnapshotKey(i64);

impl SnapshotKey {
    /// Return the database representation.
    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self.0
    }
}

// A derive cannot enforce the positive-key invariant required by database-issued identities.
impl TryFrom<i64> for SnapshotKey {
    type Error = SnapshotKeyError;

    fn try_from(value: i64) -> Result<SnapshotKey, SnapshotKeyError> {
        if value > 0 {
            Ok(SnapshotKey(value))
        } else {
            Err(SnapshotKeyError::NotPositive(value))
        }
    }
}

/// Validation failure for a persisted Snapshot key.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapshotKeyError {
    /// Database identities are strictly positive.
    #[error("snapshot key must be positive, got {0}")]
    NotPositive(i64),
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::snapshot_key::{SnapshotKey, SnapshotKeyError};

    #[test]
    fn positive_database_key_is_accepted() {
        let key = SnapshotKey::try_from(7).expect("positive keys are valid");
        pretty_assertions::assert_eq!(key.as_i64(), 7);
    }

    #[test]
    fn non_positive_database_keys_are_rejected() {
        pretty_assertions::assert_eq!(SnapshotKey::try_from(0), Err(SnapshotKeyError::NotPositive(0)));
        pretty_assertions::assert_eq!(SnapshotKey::try_from(-1), Err(SnapshotKeyError::NotPositive(-1)));
    }
}
