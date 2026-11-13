//! A Snapshot branch schema the migrations of its tenant database could not
//! bring along.

use crate::migration_set::migration_version::MigrationVersion;
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_branch::snapshot_schema_name;
use andromeda_postgresql_pool::schema_change_classification::{classify_migration, classify_schema_change};
use sqlx::migrate::MigrateError;
use thiserror::Error;

/// Why a Snapshot branch schema keeps its tenant database from the migrations
/// this binary embeds.
///
/// Every schema of a tenant database moves through the migrations together,
/// so the tenant does not open while one Snapshot branch cannot follow. The
/// migration that failed leaves nothing behind, the migrations before it stay
/// applied, and the next open of the tenant resumes from what each ledger
/// records.
#[derive(Debug, Error)]
pub enum SnapshotMigrationError {
    /// The migration ledger of the branch schema could not be read.
    #[error(
        "the migration ledger of snapshot {} in schema {} cannot be read: {source}",
        .branch.snapshot_id(),
        snapshot_schema_name(.branch.key(), .branch.branch())
    )]
    LedgerUnreadable {
        /// The Snapshot branch.
        branch: SnapshotTarget,
        /// Why the ledger could not be read.
        #[source]
        source: MigrateError,
    },
    /// The session's search path could not be set to the branch schema, so
    /// none of its migrations ran.
    #[error(
        "snapshot {} cannot put its schema {} on the search path to bring it through migration {version}: {source}",
        .branch.snapshot_id(),
        snapshot_schema_name(.branch.key(), .branch.branch())
    )]
    SearchPathNotSet {
        /// The Snapshot branch.
        branch: SnapshotTarget,
        /// The migration the branch was to be brought through.
        version: MigrationVersion,
        /// Why the search path could not be set.
        #[source]
        source: sqlx::Error,
    },
    /// The branch schema could not be brought through a migration.
    #[error(
        "snapshot {} cannot bring its schema {} through migration {version}: {source}",
        .branch.snapshot_id(),
        snapshot_schema_name(.branch.key(), .branch.branch())
    )]
    MigrationFailed {
        /// The Snapshot branch.
        branch: SnapshotTarget,
        /// The migration the branch was being brought through.
        version: MigrationVersion,
        /// Why the migration failed.
        #[source]
        source: MigrateError,
    },
    /// The branch schema took its migrations, and the session's search path
    /// could not be put back afterwards.
    #[error(
        "snapshot {} brought its schema {} through migration {version}, and the search path could not be put back: {source}",
        .branch.snapshot_id(),
        snapshot_schema_name(.branch.key(), .branch.branch())
    )]
    SearchPathNotRestored {
        /// The Snapshot branch.
        branch: SnapshotTarget,
        /// The migration the branch was brought through.
        version: MigrationVersion,
        /// Why the search path could not be put back.
        #[source]
        source: sqlx::Error,
    },
}

impl SnapshotMigrationError {
    /// The backend-neutral class of the failure: the class of the migration
    /// or statement failure behind it.
    #[must_use]
    pub fn failure_class(&self) -> PersistenceFailure {
        match self {
            SnapshotMigrationError::LedgerUnreadable { source, .. } | SnapshotMigrationError::MigrationFailed { source, .. } => classify_migration(source),
            SnapshotMigrationError::SearchPathNotSet { source, .. } | SnapshotMigrationError::SearchPathNotRestored { source, .. } => {
                classify_schema_change(source)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{lockstep_migration::snapshot_migration_error::SnapshotMigrationError, migration_set::migration_version::MigrationVersion};
    use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;
    use sqlx::{Error as DriverError, migrate::MigrateError};
    use std::io::{Error as IoError, ErrorKind};

    fn staging() -> SnapshotTarget {
        SnapshotTarget::staging(
            SnapshotKey::try_from(8).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:behind".parse().expect("a valid URI"),
        )
    }

    /// An operator repairs the branch by its schema, and finds it by its URI.
    #[test]
    fn the_message_of_a_failed_migration_names_the_snapshot_its_schema_and_the_migration() {
        let error = SnapshotMigrationError::MigrationFailed {
            branch: staging(),
            version: MigrationVersion::new(33),
            source: MigrateError::VersionMismatch(22),
        };
        let message = error.to_string();
        assert!(
            message.starts_with("snapshot urn:ngsi-ld:Snapshot:behind cannot bring its schema snap_8_stg through migration 33: "),
            "{message}"
        );
    }

    /// No migration ran, so the message names the search path as what failed.
    #[test]
    fn the_message_of_an_unset_search_path_names_the_snapshot_its_schema_and_the_search_path() {
        let error = SnapshotMigrationError::SearchPathNotSet {
            branch: staging(),
            version: MigrationVersion::new(33),
            source: DriverError::Io(IoError::from(ErrorKind::ConnectionReset)),
        };
        let message = error.to_string();
        assert!(
            message.starts_with("snapshot urn:ngsi-ld:Snapshot:behind cannot put its schema snap_8_stg on the search path to bring it through migration 33: "),
            "{message}"
        );
        assert_eq!(error.failure_class(), PersistenceFailure::Unavailable);
    }

    #[test]
    fn a_lost_connection_is_an_unreachable_store_and_a_failed_migration_a_schema_mismatch() {
        let lost = SnapshotMigrationError::LedgerUnreadable {
            branch: staging(),
            source: MigrateError::Execute(DriverError::Io(IoError::from(ErrorKind::ConnectionReset))),
        };
        assert_eq!(lost.failure_class(), PersistenceFailure::Unavailable);
        let failed = SnapshotMigrationError::MigrationFailed {
            branch: staging(),
            version: MigrationVersion::new(33),
            source: MigrateError::VersionMismatch(22),
        };
        assert_eq!(failed.failure_class(), PersistenceFailure::SchemaMismatch);
        let unrestored = SnapshotMigrationError::SearchPathNotRestored {
            branch: staging(),
            version: MigrationVersion::new(33),
            source: DriverError::Io(IoError::from(ErrorKind::ConnectionReset)),
        };
        assert_eq!(unrestored.failure_class(), PersistenceFailure::Unavailable);
    }
}
