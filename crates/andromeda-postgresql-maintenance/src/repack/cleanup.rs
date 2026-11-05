//! A run's cleanup after it stopped short of its own end.
//!
//! A run that stops before its swap cleans up on the guard's transaction.
//! The guard holds the table's `SHARE UPDATE EXCLUSIVE` lock, so its request
//! for `ACCESS EXCLUSIVE` goes ahead of a schema change waiting for the table:
//! the run's objects are dropped before the schema change reaches the table,
//! which it could not alter while the run's log table uses its row type. The
//! main session's statement is cancelled and its transaction rolled back
//! first, so its own locks do not hold the guard up.
//!
//! When the guard cannot clean up, a run that gave up on its own (a lock not
//! granted in time, a log that never converged) cleans up on its main
//! session, which still holds the table's advisory lock, and a run that
//! yielded, failed or was cancelled closes both its sessions, which ends
//! their statements and transactions, and cleans up on a fresh one once the
//! closed session has released the table's lock. A guard that did not clean
//! up rolls back first: an open guard transaction would keep its own lock in
//! the way of every other cleanup. A cleanup that does not get there leaves
//! the run's row, and the objects it names, to recovery.

use crate::{
    error::MaintenanceError,
    repack::{
        RepackSubject,
        RunSessions,
        leftover::{self, CleanupOutcome},
        lock_acquisition::{self, LockOutcome},
        table_lock::RepackTableLock,
    },
    state::repack_run,
};
use andromeda_postgresql_seaquery::{maintenance::cancel_backend, relation_lock::LockMode, repack::drop_objects, statement::Statement};
use sea_query::{Expr, Query, TableName};
use sqlx::{PgConnection, Postgres, Transaction};
use std::{
    slice,
    time::{Duration, Instant},
};
use tokio::time;
use tracing::warn;

/// How long the main session's rollback may take before the guard tries for
/// the table's lock regardless.
const MAIN_SETTLE_BUDGET: Duration = Duration::from_secs(5);

/// How long a cleanup on a fresh session waits for the stopped run's session
/// to release the table's lock.
const LOCK_RELEASE_WAIT: Duration = Duration::from_secs(10);

/// How often that wait looks again.
const LOCK_RELEASE_POLL: Duration = Duration::from_millis(500);

/// Clean up on the main session, which still holds the table's lock, and
/// delete the run's row once the objects are gone.
pub(super) async fn clean_up_on_main(
    platform: &mut PgConnection,
    main: &mut PgConnection,
    subject: RepackSubject<'_>,
) -> Result<CleanupOutcome, MaintenanceError> {
    let cleanup = leftover::clean_up(&mut *main, subject.relid).await?;
    RepackTableLock::of(subject.relid).release(main).await?;
    end_after_cleanup(platform, subject, cleanup).await
}

/// Clean up after a setup that failed, on the main session, which still
/// holds the table's lock: the setup may have committed before the guard's
/// lock failed, and its row is written either way. `None` when the cleanup
/// failed too, which leaves the objects and the row to recovery.
pub(super) async fn clean_up_after_failed_setup(platform: &mut PgConnection, main: &mut PgConnection, subject: RepackSubject<'_>) -> Option<CleanupOutcome> {
    match clean_up_on_main(platform, main, subject).await {
        Ok(cleanup) => Some(cleanup),
        Err(error) => {
            warn!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, target = subject.heap.relation_name(), error = %error, decision = "repack-cleanup-deferred", "PostgreSQL repack left its objects for recovery");
            None
        }
    }
}

/// Roll the guard's transaction back, ending the lock it holds. A failed
/// rollback is logged: closing the session ends the transaction too.
pub(super) async fn roll_back_guard(guard: Transaction<'_, Postgres>, subject: RepackSubject<'_>) {
    if let Err(error) = guard.rollback().await {
        warn!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, target = subject.heap.relation_name(), error = %error, decision = "repack-guard-rollback-failed", "PostgreSQL repack guard keeps its transaction until its session closes");
    }
}

/// Close the run's sessions, ending their transactions and statements, and
/// clean up on a fresh session once the table's lock is free.
pub(super) async fn clean_up_on_fresh_session(
    platform: &mut PgConnection,
    sessions: RunSessions,
    subject: RepackSubject<'_>,
) -> Result<CleanupOutcome, MaintenanceError> {
    let RunSessions { pool, .. } = sessions;
    let mut session = pool.acquire().await?;
    session.close_on_drop();
    let waiting = Instant::now();
    let cleanup = loop {
        let cleanup = leftover::clean_up(&mut session, subject.relid).await?;
        if cleanup != CleanupOutcome::HeldElsewhere || waiting.elapsed() >= LOCK_RELEASE_WAIT {
            break cleanup;
        }
        time::sleep(LOCK_RELEASE_POLL).await;
    };
    end_after_cleanup(platform, subject, cleanup).await
}

/// Delete the run's row once its objects are gone; a cleanup that did not
/// get there leaves the row for recovery.
async fn end_after_cleanup(platform: &mut PgConnection, subject: RepackSubject<'_>, cleanup: CleanupOutcome) -> Result<CleanupOutcome, MaintenanceError> {
    match cleanup {
        CleanupOutcome::Cleaned | CleanupOutcome::Vanished => repack_run::record_end(platform, subject.tenant.key, subject.row).await?,
        CleanupOutcome::HeldElsewhere | CleanupOutcome::LockWait => {
            warn!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, branch = ?subject.row.branch(), target = subject.heap.relation_name(), cleanup = ?cleanup, decision = "repack-cleanup-deferred", "PostgreSQL repack left its objects for recovery");
        }
    }
    Ok(cleanup)
}

/// Drop the run's objects from the guard's transaction, after cancelling the
/// main session's statement and letting its transaction roll back. `None`
/// when a statement of the cleanup failed, which leaves the cleanup to the
/// fallback; the guard's transaction is rolled back unless it committed.
pub(super) async fn clean_up_on_guard(
    mut guard: Transaction<'_, Postgres>,
    main: &mut PgConnection,
    main_pid: i32,
    subject: RepackSubject<'_>,
    table: &TableName,
) -> Option<CleanupOutcome> {
    match guard_cleanup(&mut guard, main, main_pid, subject.relid, table).await {
        Ok(LockOutcome::Acquired) => match guard.commit().await {
            Ok(()) => Some(CleanupOutcome::Cleaned),
            Err(error) => report_guard_failure(subject, &MaintenanceError::from(error)),
        },
        Ok(LockOutcome::Abandoned) => {
            roll_back_guard(guard, subject).await;
            Some(CleanupOutcome::LockWait)
        }
        Err(error) => {
            roll_back_guard(guard, subject).await;
            report_guard_failure(subject, &error)
        }
    }
}

/// The statements of the guard's cleanup: the main session settled, the
/// table's `ACCESS EXCLUSIVE` lock, and the run's objects dropped.
async fn guard_cleanup(
    guard: &mut PgConnection,
    main: &mut PgConnection,
    main_pid: i32,
    relid: u32,
    table: &TableName,
) -> Result<LockOutcome, MaintenanceError> {
    let cancel = Query::select().expr(cancel_backend(Expr::val(main_pid))).to_owned();
    cancel.to_query_scalar::<bool>().fetch_one(&mut *guard).await?;
    // The main session's dropped transaction rolls back ahead of its next
    // statement, which releases the locks it holds on the table.
    let settle = Query::select().expr(Expr::val(1)).to_owned();
    match time::timeout(MAIN_SETTLE_BUDGET, settle.to_query_scalar::<i32>().fetch_one(main)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            warn!(error = %error, decision = "repack-main-unsettled", "PostgreSQL repack main session did not settle; the guard tries for the lock regardless");
        }
        Err(elapsed) => {
            warn!(error = %elapsed, decision = "repack-main-unsettled", "PostgreSQL repack main session did not settle; the guard tries for the lock regardless");
        }
    }
    let locked = lock_acquisition::acquire(guard, slice::from_ref(table), LockMode::AccessExclusive, None).await?;
    if locked == LockOutcome::Acquired {
        drop_objects(relid).to_query().execute(guard).await?;
    }
    Ok(locked)
}

/// Log a guard cleanup that failed, leaving it to the fallback.
fn report_guard_failure(subject: RepackSubject<'_>, error: &MaintenanceError) -> Option<CleanupOutcome> {
    warn!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, target = subject.heap.relation_name(), error = %error, decision = "repack-guard-cleanup-failed", "PostgreSQL repack cleans up on another session");
    None
}

/// Delete the run's row and release the table's advisory lock after the
/// guard cleaned up.
pub(super) async fn end_after_guard_cleanup(
    platform: &mut PgConnection,
    main: &mut PgConnection,
    subject: RepackSubject<'_>,
) -> Result<CleanupOutcome, MaintenanceError> {
    repack_run::record_end(platform, subject.tenant.key, subject.row).await?;
    if let Err(error) = RepackTableLock::of(subject.relid).release(main).await {
        warn!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, target = subject.heap.relation_name(), error = %error, decision = "repack-lock-left", "PostgreSQL repack leaves its table lock to the end of its session");
    }
    Ok(CleanupOutcome::Cleaned)
}
