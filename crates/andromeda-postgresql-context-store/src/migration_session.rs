//! The one database session every migrator of a migration run runs in.
//!
//! sqlx takes its advisory lock for each run of a migrator and leaves it held
//! when the run fails, so a migration that is run again after a refused lock
//! takes the lock twice and releases it once. A lock left held by one session
//! would stall every later run on another session; on the session holding it,
//! the lock is taken again rather than waited for, and closing the session
//! releases what is left of it. Every migrator of a run (the platform set's,
//! `public`'s and every Snapshot branch's) therefore runs on one connection
//! detached from the pool, closed when the run ends, so no setting it takes
//! reaches a connection the pool lends out.
//!
//! A run under another search path, as a Snapshot branch's migrations need,
//! sets it for the session, because each migration commits in a transaction
//! of its own, and the search path the session opened with is put back
//! afterwards, so a later run in `public` resolves its relations there.
//!
//! The connection waits a bounded time for each lock and never has a statement
//! cut short, as every connection that changes the schema does, and a
//! migration refused a lock is rolled back whole, so it is run again a bounded
//! number of times.

use crate::{
    error::ContextStoreError,
    migration_set::migration_version::MigrationVersion,
    search_path::{current_search_path, steer_search_path},
};
use andromeda_postgresql_pool::schema_change_lock::{SCHEMA_CHANGE_LOCK_RETRY, is_lock_not_available};
use andromeda_postgresql_seaquery::{search_path::SearchPath, setting::SettingScope};
use andromeda_retry::tokio_retry_delay::TokioRetryDelay;
use sqlx::{
    Connection,
    PgConnection,
    PgPool,
    migrate::{MigrateError, Migrator},
};

/// The session a migration run's migrators run in.
pub(crate) struct MigrationSession {
    /// The connection, detached from the pool it was opened from.
    connection: PgConnection,
    /// The search path the session opened with, which every run in `public`
    /// resolves under.
    search_path: SearchPath,
}

impl MigrationSession {
    /// Detach one connection from `pool` for a migration run.
    ///
    /// `pool` must reach PostgreSQL directly, never through a
    /// transaction-pooling proxy: the advisory lock and the search path hold
    /// for the session.
    pub(crate) async fn open(pool: &PgPool) -> Result<MigrationSession, ContextStoreError> {
        let mut connection = pool.acquire().await?.detach();
        let search_path = current_search_path(&mut connection).await?;
        Ok(MigrationSession { connection, search_path })
    }

    /// The session's connection, for a read between two runs.
    pub(crate) const fn connection(&mut self) -> &mut PgConnection {
        &mut self.connection
    }

    /// Run `migrator` through the migration `through`, or through its last
    /// one when `None`, running it again while a migration is refused a lock.
    pub(crate) async fn run(&mut self, migrator: &Migrator, through: Option<MigrationVersion>) -> Result<(), MigrateError> {
        let connection = &mut self.connection;
        SCHEMA_CHANGE_LOCK_RETRY
            .run(
                &TokioRetryDelay,
                |error, _| migration_lock_not_available(error),
                async |_| migrator.run_direct(through.map(MigrationVersion::get), &mut *connection, false).await,
            )
            .await
    }

    /// Run `migrator` through the migration `through` with `search_path` as
    /// the session's search path, then put the session's own back.
    ///
    /// The search path is put back whether or not the migrations succeed, so
    /// no later run of the session resolves its relations, its ledger
    /// included, under `search_path`. When `search_path` cannot be set or the
    /// migrations fail, that failure is the one returned.
    pub(crate) async fn run_under(&mut self, search_path: &SearchPath, migrator: &Migrator, through: MigrationVersion) -> Result<(), RunUnderError> {
        let migrated = match steer_search_path(&mut self.connection, search_path, SettingScope::Session).await {
            Ok(()) => self.run(migrator, Some(through)).await.map_err(RunUnderError::Migration),
            Err(error) => Err(RunUnderError::SearchPathNotSet(error)),
        };
        let restored = steer_search_path(&mut self.connection, &self.search_path, SettingScope::Session).await;
        run_under_outcome(migrated, restored)
    }

    /// End the session, releasing every lock it holds.
    pub(crate) async fn close(self) -> Result<(), ContextStoreError> {
        Ok(self.connection.close().await?)
    }
}

/// Why a run under another search path failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RunUnderError {
    /// The search path could not be set, so no migration ran.
    #[error("the session's search path could not be set: {0}")]
    SearchPathNotSet(#[source] sqlx::Error),
    /// The migrations failed, and the one that failed committed nothing.
    #[error(transparent)]
    Migration(MigrateError),
    /// The migrations committed, and the session's search path could not be
    /// put back afterwards.
    #[error("the session's search path could not be put back: {0}")]
    SearchPathNotRestored(#[source] sqlx::Error),
}

/// The outcome of a run under another search path, from the outcome of the
/// run (setting the search path, then the migrations) and of putting the
/// session's search path back: the run's failure first, then the restore's, a
/// restore that fails after a failed run only logged.
fn run_under_outcome(ran: Result<(), RunUnderError>, restored: Result<(), sqlx::Error>) -> Result<(), RunUnderError> {
    match (ran, restored) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(RunUnderError::SearchPathNotRestored(error)),
        (Err(failure), Ok(())) => Err(failure),
        (Err(failure), Err(error)) => {
            tracing::warn!(%error, event = "migration_search_path_not_restored", "the session's search path could not be put back after a failed run");
            Err(failure)
        }
    }
}

/// Whether a migration failed because a statement was refused a lock.
fn migration_lock_not_available(error: &MigrateError) -> bool {
    matches!(error, MigrateError::Execute(source) | MigrateError::ExecuteMigration(source, _) if is_lock_not_available(source))
}

#[cfg(test)]
mod tests {
    use crate::migration_session::{RunUnderError, migration_lock_not_available, run_under_outcome};
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use sqlx::{Error as DriverError, migrate::MigrateError};

    /// Migrations that committed are not reported as failed when only the
    /// restore after them fails.
    #[test]
    fn a_run_under_another_search_path_succeeds_only_when_both_its_migrations_and_the_restore_do() {
        assert!(run_under_outcome(Ok(()), Ok(())).is_ok());
        assert!(matches!(
            run_under_outcome(Ok(()), Err(DriverError::PoolTimedOut)),
            Err(RunUnderError::SearchPathNotRestored(DriverError::PoolTimedOut))
        ));
    }

    /// The run's failure names what went wrong in the branch; a restore
    /// failing after it is a consequence, so it is logged, not returned.
    #[test]
    fn a_failed_run_reports_the_migrations_failure_whether_or_not_the_restore_succeeds() {
        let failed = || Err(RunUnderError::Migration(MigrateError::VersionMismatch(33)));
        assert!(matches!(
            run_under_outcome(failed(), Ok(())),
            Err(RunUnderError::Migration(MigrateError::VersionMismatch(33)))
        ));
        assert!(matches!(
            run_under_outcome(failed(), Err(DriverError::PoolTimedOut)),
            Err(RunUnderError::Migration(MigrateError::VersionMismatch(33)))
        ));
    }

    /// A search path that could not be set ran no migration, which the
    /// outcome says rather than reporting a failed migration.
    #[test]
    fn a_search_path_that_could_not_be_set_is_reported_as_such_whether_or_not_the_restore_succeeds() {
        let unset = || Err(RunUnderError::SearchPathNotSet(DriverError::PoolTimedOut));
        assert!(matches!(
            run_under_outcome(unset(), Ok(())),
            Err(RunUnderError::SearchPathNotSet(DriverError::PoolTimedOut))
        ));
        assert!(matches!(
            run_under_outcome(unset(), Err(DriverError::RowNotFound)),
            Err(RunUnderError::SearchPathNotSet(DriverError::PoolTimedOut))
        ));
    }

    /// The lock classification itself is the pool crate's; what is decided
    /// here is which migration failures carry a statement error to classify.
    #[test]
    fn a_migration_failure_without_a_refused_lock_is_not_run_again() {
        assert!(!migration_lock_not_available(&MigrateError::Execute(DriverError::PoolTimedOut)));
        assert!(!migration_lock_not_available(&MigrateError::ExecuteMigration(DriverError::RowNotFound, 30)));
        assert!(!migration_lock_not_available(&MigrateError::VersionMismatch(30)));
        assert!(!migration_lock_not_available(&MigrateError::Dirty(30)));
    }

    #[test]
    fn a_migration_refused_a_lock_is_run_again() {
        let refused = || FakeDatabaseError::new("lock timeout", "55P03").into_sqlx();
        assert!(migration_lock_not_available(&MigrateError::Execute(refused())));
        assert!(migration_lock_not_available(&MigrateError::ExecuteMigration(refused(), 33)));
    }
}
