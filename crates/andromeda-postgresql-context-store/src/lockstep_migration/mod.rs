//! Migrating every schema of a tenant database in version lockstep.
//!
//! Every branch, the live one in `public` and each Snapshot's in its own
//! schema, stores values of the composite types `public` owns, and those types
//! exist in one version only: a tenant migration that changes a type changes
//! the values every branch holds at once. A branch migration must therefore
//! have run in every branch before any later migration runs anywhere, or a
//! Snapshot branch would be migrated against types already past it. The walk
//! goes through the tenant set in version order: a tenant migration runs in
//! `public`, and a branch migration runs in `public` and then in every Snapshot
//! branch schema before the walk goes on. Each migration commits in each schema
//! in a transaction of its own, so no transaction holds the locks of every
//! Snapshot at once.
//!
//! The walk is decided from what the ledgers record, so a run cut short
//! anywhere resumes where it stopped: a Snapshot branch behind a branch
//! migration `public` already applied is brought through it first. A Snapshot
//! branch that cannot take a migration stops the walk, before any later
//! migration runs anywhere, with an error naming the branch.

/// The steps that bring every schema through the tenant set, decided from
/// what the ledgers record.
mod plan;
/// A Snapshot branch schema the migrations could not bring along.
pub mod snapshot_migration_error;
/// What a tenant database's ledgers record.
mod survey;

use crate::{
    error::ContextStoreError,
    lockstep_migration::{
        plan::{LockstepStep, lockstep_plan},
        snapshot_migration_error::SnapshotMigrationError,
        survey::TenantSurvey,
    },
    migration_session::{MigrationSession, RunUnderError},
    migration_set::{
        branch::branch_migrator,
        migration_version::MigrationVersion,
        tenant::{tenant_migrator, tenant_set_migrations},
    },
};
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_postgresql_branch::snapshot_search_path;

/// Bring `public` and every Snapshot branch schema of the database `session`
/// is attached to through the tenant set this binary embeds.
///
/// # Errors
///
/// Returns [`ContextStoreError::Migration`] when a migration fails in
/// `public`, [`ContextStoreError::SnapshotMigration`] naming the branch when a
/// Snapshot branch cannot be read or migrated, and the failures of reading
/// the ledgers and the Snapshot control records.
pub(crate) async fn migrate_in_lockstep(session: &mut MigrationSession) -> Result<(), ContextStoreError> {
    let survey = TenantSurvey::read(session.connection()).await?;
    for step in lockstep_plan(&tenant_set_migrations(), survey.public_applied(), survey.snapshot_progress()) {
        match step {
            LockstepStep::Public(version) => session.run(tenant_migrator(), Some(version)).await?,
            LockstepStep::Snapshots(version) => {
                for branch in survey.snapshot_branches() {
                    migrate_snapshot_branch(session, branch, version).await?;
                }
                tracing::info!(
                    migration = version.get(),
                    snapshot_branches = survey.snapshot_branches().len(),
                    event = "snapshot_branches_migrated",
                    "every Snapshot branch schema was brought through a branch migration"
                );
            }
        }
    }
    Ok(())
}

/// Bring one Snapshot branch schema through the branch migration `through`,
/// with the branch schema first on the session's search path.
async fn migrate_snapshot_branch(session: &mut MigrationSession, branch: &SnapshotTarget, through: MigrationVersion) -> Result<(), ContextStoreError> {
    let search_path = snapshot_search_path(branch.key(), branch.branch());
    session
        .run_under(&search_path, &branch_migrator(branch.key(), branch.branch()), through)
        .await
        .map_err(|failure| branch_failure(branch, through, failure).into())
}

/// The failure of bringing `branch` through the migration `through`, naming
/// the branch.
fn branch_failure(branch: &SnapshotTarget, through: MigrationVersion, failure: RunUnderError) -> SnapshotMigrationError {
    // clone: the error owns the branch it names.
    let branch = branch.clone();
    match failure {
        RunUnderError::SearchPathNotSet(source) => SnapshotMigrationError::SearchPathNotSet {
            branch,
            version: through,
            source,
        },
        RunUnderError::Migration(source) => SnapshotMigrationError::MigrationFailed {
            branch,
            version: through,
            source,
        },
        RunUnderError::SearchPathNotRestored(source) => SnapshotMigrationError::SearchPathNotRestored {
            branch,
            version: through,
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        lockstep_migration::{branch_failure, snapshot_migration_error::SnapshotMigrationError},
        migration_session::RunUnderError,
        migration_set::migration_version::MigrationVersion,
    };
    use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use pretty_assertions::assert_eq;
    use sqlx::{Error as DriverError, migrate::MigrateError};

    fn published() -> SnapshotTarget {
        SnapshotTarget::published(
            SnapshotKey::try_from(4).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:failing".parse().expect("a valid URI"),
        )
    }

    const THROUGH: MigrationVersion = MigrationVersion::new(33);

    #[test]
    fn a_search_path_that_could_not_be_set_names_the_branch_and_reports_no_failed_migration() {
        match branch_failure(&published(), THROUGH, RunUnderError::SearchPathNotSet(DriverError::PoolTimedOut)) {
            SnapshotMigrationError::SearchPathNotSet { branch, version, .. } => assert_eq!((branch, version), (published(), THROUGH)),
            other @ (SnapshotMigrationError::LedgerUnreadable { .. }
            | SnapshotMigrationError::MigrationFailed { .. }
            | SnapshotMigrationError::SearchPathNotRestored { .. }) => panic!("the search path was never set: {other}"),
        }
    }

    #[test]
    fn a_failed_migration_names_the_branch_and_keeps_the_migration_failure() {
        match branch_failure(&published(), THROUGH, RunUnderError::Migration(MigrateError::VersionMismatch(22))) {
            SnapshotMigrationError::MigrationFailed { branch, version, source } => {
                assert_eq!((branch, version), (published(), THROUGH));
                assert!(matches!(source, MigrateError::VersionMismatch(22)), "{source}");
            }
            other @ (SnapshotMigrationError::LedgerUnreadable { .. }
            | SnapshotMigrationError::SearchPathNotSet { .. }
            | SnapshotMigrationError::SearchPathNotRestored { .. }) => panic!("a migration failed: {other}"),
        }
    }

    #[test]
    fn a_search_path_that_could_not_be_put_back_names_the_branch_it_was_set_for() {
        match branch_failure(&published(), THROUGH, RunUnderError::SearchPathNotRestored(DriverError::PoolTimedOut)) {
            SnapshotMigrationError::SearchPathNotRestored { branch, version, .. } => assert_eq!((branch, version), (published(), THROUGH)),
            other @ (SnapshotMigrationError::LedgerUnreadable { .. }
            | SnapshotMigrationError::SearchPathNotSet { .. }
            | SnapshotMigrationError::MigrationFailed { .. }) => panic!("the migrations committed: {other}"),
        }
    }
}
