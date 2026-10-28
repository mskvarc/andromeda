//! The value `snapshot_preparation.branch_schema_version` stores.
//!
//! The statements that select Snapshots by whether their branch schemas are
//! current, and the migration set that produces the number, name the same
//! type, so it lives with the column it is stored in.

use derive_more::Display;
use thiserror::Error;

/// The highest branch migration a Snapshot's schemas carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
pub struct BranchSchemaVersion(i32);

impl BranchSchemaVersion {
    /// The value as the `snapshot_preparation.branch_schema_version` column stores it.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl TryFrom<i64> for BranchSchemaVersion {
    type Error = BranchVersionError;

    /// Accept a migration version as a branch schema version: positive and
    /// within the stored `INTEGER` range.
    fn try_from(version: i64) -> Result<BranchSchemaVersion, BranchVersionError> {
        i32::try_from(version)
            .ok()
            .filter(|version| *version > 0)
            .map(BranchSchemaVersion)
            .ok_or(BranchVersionError::OutOfRange(version))
    }
}

impl TryFrom<i32> for BranchSchemaVersion {
    type Error = BranchVersionError;

    /// Read back what `snapshot_preparation.branch_schema_version` stores.
    fn try_from(stored: i32) -> Result<BranchSchemaVersion, BranchVersionError> {
        BranchSchemaVersion::try_from(i64::from(stored))
    }
}

/// Why a branch schema version could not be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BranchVersionError {
    /// The binary embeds no branch migration.
    #[error("the embedded branch migration set is empty")]
    NoMigrations,
    /// A version is not a positive `INTEGER`.
    #[error("branch schema version {0} is not a positive 32-bit integer")]
    OutOfRange(i64),
}

#[cfg(test)]
mod tests {
    use crate::branch_schema_version::{BranchSchemaVersion, BranchVersionError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_version_outside_the_positive_integer_range_is_refused() {
        assert_eq!(BranchSchemaVersion::try_from(0_i64), Err(BranchVersionError::OutOfRange(0)));
        assert_eq!(BranchSchemaVersion::try_from(-3_i32), Err(BranchVersionError::OutOfRange(-3)));
        assert_eq!(
            BranchSchemaVersion::try_from(i64::from(i32::MAX) + 1),
            Err(BranchVersionError::OutOfRange(i64::from(i32::MAX) + 1))
        );
        assert_eq!(BranchSchemaVersion::try_from(21_i64).map(BranchSchemaVersion::get), Ok(21));
    }
}
