//! One online repack of one heap, driven in process over the `pg_repack`
//! extension's SQL interface.
//!
//! The extension generates every statement specific to the table and does
//! the swap; the run sequences the phases, takes every lock in bounded
//! attempts, yields to any session that needs the table, bounds each long
//! statement by the phase budget and cleans up after itself. A run uses two
//! sessions of its own: the main session copies, builds and replays, and the
//! guard session holds the table's `SHARE UPDATE EXCLUSIVE` lock from the
//! setup to the swap, so no schema change lands while the copy is made, and
//! watches the table for sessions waiting on it. Both sessions check their
//! client every second, so a broker that stops mid-statement stops the
//! statement too.

mod cleanup;
mod copy;
mod guard_session;
/// The objects an interrupted repack leaves, and their cleanup.
pub(crate) mod leftover;
mod lock_acquisition;
mod log_replay;
mod old_transactions;
/// The phases one repack runs through.
pub(crate) mod phase;
/// How long a repack's long statements may run.
pub(crate) mod phase_budget;
mod run_phases;
mod setup;
mod swap;
/// The advisory lock that keeps one table to one repack at a time.
pub(crate) mod table_lock;
mod table_script;
/// The `pg_repack` version a tenant database runs.
pub(crate) mod version;
mod yield_watch;

use crate::{
    branch::BranchTarget,
    candidate::heap_target::HeapTarget,
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    inspection,
    repack::{
        leftover::CleanupOutcome,
        phase::RepackPhase,
        run_phases::CopyPlan,
        setup::SetupOutcome,
        table_lock::{RepackTableLock, TableLockTaken},
        table_script::ScriptAbsence,
    },
    state::{RegisteredTenant, repack_run},
};
use andromeda_postgresql_seaquery::{
    command::Command,
    maintenance::{backend_pid, to_regclass, total_relation_size},
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
    transaction::{Isolation, TransactionStart},
};
use andromeda_telemetry::maintenance::record_repack_phase;
use sea_query::{Expr, Query, TableName};
use sqlx::{PgConnection, PgPool, Postgres, pool::PoolConnection};
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// The table one run repacks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RepackSubject<'s> {
    /// The tenant whose database holds the table.
    pub(crate) tenant: &'s RegisteredTenant,
    /// The repack target, in its branch.
    pub(crate) row: BranchTarget,
    /// The heap the target repacks.
    pub(crate) heap: HeapTarget,
    /// The table's OID.
    pub(crate) relid: u32,
}

/// Why a run was abandoned.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum RepackAbandonment {
    /// An `ACCESS EXCLUSIVE` lock was not granted within the wait budget.
    LockWait,
    /// Writes kept the log from converging, or a transaction older than the
    /// copy stayed open past the wait budget.
    NotConverging,
    /// A session waited for the table, and the run gave way to it.
    Yielded,
    /// Another session holds the table's repack lock.
    TableLocked,
    /// The table carries a trigger named as the run's.
    ConflictingTrigger,
    /// The table has an invalid index, which a copy would not rebuild.
    InvalidIndex,
    /// The extension has no statements a run could execute for the table.
    ScriptAbsent(ScriptAbsence),
}

/// How one run ended.
#[derive(Debug)]
pub(crate) enum RepackOutcome {
    /// The table holds the copy's files and the run's objects are dropped.
    Repacked {
        /// The relation's total bytes before the run.
        before_bytes: u64,
        /// The relation's total bytes after it.
        after_bytes: u64,
    },
    /// The run gave up, and its cleanup ended as given.
    Abandoned {
        /// Why the run gave up.
        reason: RepackAbandonment,
        /// How its cleanup ended, `None` when the run left nothing behind.
        cleanup: Option<CleanupOutcome>,
    },
    /// A statement of the run failed, and its cleanup ended as given.
    Failed {
        /// The failure.
        error: MaintenanceError,
        /// How its cleanup ended, `None` when the run left nothing behind.
        cleanup: Option<CleanupOutcome>,
    },
    /// Cooperative cancellation stopped the run, and its cleanup ended as
    /// given.
    Cancelled {
        /// How its cleanup ended, `None` when the run left nothing behind or
        /// the shutdown budget ran out first.
        cleanup: Option<CleanupOutcome>,
    },
}

/// The two sessions a run works through.
pub(crate) struct RunSessions {
    /// The main session: setup, copy, index builds, replay and drop.
    main: PoolConnection<Postgres>,
    /// The guard session: the table's lock from setup to swap, and the
    /// yield watch.
    guard: PoolConnection<Postgres>,
    /// The main session's backend.
    main_pid: i32,
    /// The guard session's backend.
    guard_pid: i32,
    /// The pool both sessions came from, a cleanup's fresh session comes
    /// from.
    pool: PgPool,
}

impl RunSessions {
    /// The main session, on which a run is prepared before it starts: the
    /// Snapshot branch lock it takes there is held until the run ends.
    pub(crate) fn main(&mut self) -> &mut PgConnection {
        &mut self.main
    }
}

/// Repack the subject's table through `sessions`, recording each phase on
/// `platform`. A cleanup after a failure opens a fresh session from the
/// sessions' pool.
///
/// # Errors
///
/// Returns [`MaintenanceError`] when a platform statement fails or a session
/// cannot be opened. A failure inside the tenant database ends the run as
/// [`RepackOutcome::Failed`] after its cleanup.
pub(crate) async fn run(
    platform: &mut PgConnection,
    mut sessions: RunSessions,
    subject: RepackSubject<'_>,
    cancellation: &CancellationToken,
) -> Result<RepackOutcome, MaintenanceError> {
    let lock = RepackTableLock::of(subject.relid);
    if lock.try_take(&mut sessions.main).await? == TableLockTaken::HeldElsewhere {
        return Ok(RepackOutcome::Abandoned {
            reason: RepackAbandonment::TableLocked,
            cleanup: None,
        });
    }
    let branch = subject.row.branch();
    let table = TableName::from((branch.schema(), subject.heap.table()));
    let before_bytes = relation_bytes(&mut sessions.main, &branch.qualified(subject.heap.relation_name())).await?;
    let started = Instant::now();
    // The guard's transaction opens before the setup, so the lock the guard
    // queues for during the setup is held until the swap commits it.
    let mut guard = TransactionStart::at(Isolation::ReadCommitted).begin_on(&mut sessions.guard).await?;
    let setup = setup::set_up(platform, &mut sessions.main, &mut guard, sessions.guard_pid, subject, &table).await;
    let script = match setup {
        Ok(SetupOutcome::Ready(script)) => script,
        Ok(SetupOutcome::Abandoned(reason)) => {
            guard.rollback().await?;
            lock.release(&mut sessions.main).await?;
            return Ok(RepackOutcome::Abandoned { reason, cleanup: None });
        }
        Err(error) => {
            cleanup::roll_back_guard(guard, subject).await;
            let cleanup = cleanup::clean_up_after_failed_setup(platform, &mut sessions.main, subject).await;
            return Ok(RepackOutcome::Failed { error, cleanup });
        }
    };
    record_repack_phase(RepackPhase::Setup.label(), started.elapsed());
    let plan = CopyPlan::new(
        subject,
        &script,
        &table,
        phase_budget::phase_budget(before_bytes),
        sessions.main_pid,
        sessions.guard_pid,
    );
    if let Err(stopped) = run_phases::copy_and_swap(platform, guard, &mut sessions.main, plan, cancellation).await {
        return run_phases::finish_stopped(platform, sessions, subject, stopped).await;
    }
    repack_run::record_end(platform, subject.tenant.key, subject.row).await?;
    analyze(&mut sessions.main, subject).await;
    let after_bytes = relation_bytes(&mut sessions.main, &branch.qualified(subject.heap.relation_name())).await?;
    lock.release(&mut sessions.main).await?;
    Ok(RepackOutcome::Repacked { before_bytes, after_bytes })
}

/// Open and configure a run's two sessions.
pub(crate) async fn open_sessions(pool: &PgPool) -> Result<RunSessions, MaintenanceError> {
    let mut main = pool.acquire().await?;
    main.close_on_drop();
    let mut guard = pool.acquire().await?;
    guard.close_on_drop();
    for session in [&mut main, &mut guard] {
        inspection::configure_session(session).await?;
        SettingAssignment::new(Setting::ClientConnectionCheckInterval, "1s", SettingScope::Session)
            .to_query()
            .execute(&mut **session)
            .await?;
    }
    let main_pid = Query::select()
        .expr(backend_pid())
        .to_owned()
        .to_query_scalar::<i32>()
        .fetch_one(&mut *main)
        .await?;
    let guard_pid = guard_session::prepare_guard(&mut guard).await?;
    Ok(RunSessions {
        main,
        guard,
        main_pid,
        guard_pid,
        // The pool is a shared handle; the sessions keep one for a cleanup.
        pool: pool.clone(),
    })
}

/// The total bytes of `relation`, its indexes and TOAST table included, zero
/// when it does not resolve.
async fn relation_bytes(connection: &mut PgConnection, relation: &str) -> Result<u64, MaintenanceError> {
    let statement = Query::select().expr(total_relation_size(to_regclass(Expr::val(relation)))).to_owned();
    let bytes = statement.to_query_scalar::<Option<i64>>().fetch_one(connection).await?;
    bytes.map_or(Ok(0), |bytes| non_negative(bytes, CatalogCounter::RelationBytes))
}

/// Refresh the repacked table's planner statistics. The table is already
/// repacked, so a failure is logged and leaves the outcome as it is.
async fn analyze(connection: &mut PgConnection, subject: RepackSubject<'_>) {
    let started = Instant::now();
    match subject.heap.analyze_statement(subject.row.branch()).to_query().execute(connection).await {
        Ok(_) => record_repack_phase(RepackPhase::Analyze.label(), started.elapsed()),
        Err(error) => {
            warn!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, branch = ?subject.row.branch(), target = subject.heap.relation_name(), error = %error, decision = "repack-analyze-failed", "PostgreSQL repack left the table's planner statistics to autoanalyze");
        }
    }
}
