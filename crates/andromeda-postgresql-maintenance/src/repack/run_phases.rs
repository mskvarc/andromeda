//! The phases after a run's setup, and how a run that stops short of its end
//! ends.
//!
//! The copy, the index builds and the replay run under the yield watch on
//! the guard session and under cancellation; the swap and the drop run under
//! cancellation alone. A run that stops before its swap, or whose swap is
//! not granted its lock, is cleaned up on the guard's transaction first;
//! when that does not get there, the run is cleaned up on its main session
//! or on a fresh one. A cancelled run's whole cleanup shares one budget.

use crate::{
    error::MaintenanceError,
    repack::{
        RepackAbandonment,
        RepackOutcome,
        RepackSubject,
        RunSessions,
        cleanup,
        copy::{self, CopyOutcome},
        leftover::CleanupOutcome,
        phase::RepackPhase,
        swap::{self, PhaseEnd},
        table_script::RepackTableScript,
        yield_watch,
    },
    state::repack_run,
};
use andromeda_telemetry::maintenance::record_repack_phase;
use sea_query::TableName;
use sqlx::{PgConnection, Postgres, Transaction};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::time::{self, Instant as Deadline};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// How long a cancelled run's cleanup may take before the broker stops.
const SHUTDOWN_CLEANUP_BUDGET: Duration = Duration::from_secs(30);

/// Why the phases after the setup stopped short of their own end.
#[derive(derive_more::From)]
pub(super) enum RunStop {
    /// The run gave up on its own.
    #[from(ignore)]
    Abandoned(RepackAbandonment),
    /// The yield watch cancelled a phase.
    #[from(ignore)]
    Yielded,
    /// Cooperative cancellation stopped a phase.
    #[from(ignore)]
    Cancelled,
    /// A statement failed.
    Failed(MaintenanceError),
}

/// A run stopped after its setup, with what the guard's cleanup came to.
pub(super) struct Stopped {
    /// Why the run stopped.
    stop: RunStop,
    /// How the guard's cleanup ended, `None` when the guard could not clean
    /// up.
    cleanup: Option<CleanupOutcome>,
    /// When a cancelled run's cleanup must have ended.
    deadline: Deadline,
}

impl Stopped {
    /// A run stopped where the guard has no cleanup to offer.
    fn without_guard(stop: RunStop) -> Stopped {
        Stopped {
            stop,
            cleanup: None,
            deadline: Deadline::now() + SHUTDOWN_CLEANUP_BUDGET,
        }
    }
}

/// Copy, build and replay under the yield watch, then swap and drop, each
/// phase recorded and timed, the guard's transaction `guard` holding the
/// table's lock from the setup until the swap commits it. A run that stops
/// before the swap, or whose swap is not granted its lock, is cleaned up on
/// the guard's transaction, whose lock keeps a waiting schema change from
/// reaching the table while the run's log table still uses its row type.
pub(super) async fn copy_and_swap(
    platform: &mut PgConnection,
    mut guard: Transaction<'_, Postgres>,
    main: &mut PgConnection,
    plan: CopyPlan<'_>,
    cancellation: &CancellationToken,
) -> Result<(), Stopped> {
    let CopyPlan { subject, script, table, .. } = plan;
    if let Err(stop) = copy_build_replay(platform, &mut guard, main, plan, cancellation).await {
        return Err(clean_up_on_guard(guard, main, plan, stop).await);
    }
    begin_phase(platform, subject, RepackPhase::Swap)
        .await
        .map_err(|error| Stopped::without_guard(RunStop::Failed(error)))?;
    let started = Instant::now();
    match cancellable(cancellation, swap::swap_files(&mut guard, main, script, table)).await {
        Ok(PhaseEnd::Done) => {}
        Ok(PhaseEnd::Abandoned(reason)) => return Err(clean_up_on_guard(guard, main, plan, RunStop::Abandoned(reason)).await),
        Err(stop) => return Err(Stopped::without_guard(stop)),
    }
    guard
        .commit()
        .await
        .map_err(|error| Stopped::without_guard(RunStop::Failed(MaintenanceError::from(error))))?;
    record_repack_phase(RepackPhase::Swap.label(), started.elapsed());
    let stopped = Stopped::without_guard;
    begin_phase(platform, subject, RepackPhase::Drop)
        .await
        .map_err(|error| stopped(RunStop::Failed(error)))?;
    let started = Instant::now();
    match cancellable(cancellation, swap::drop_old_files(main, subject.relid, table))
        .await
        .map_err(stopped)?
    {
        PhaseEnd::Done => record_repack_phase(RepackPhase::Drop.label(), started.elapsed()),
        PhaseEnd::Abandoned(reason) => return Err(stopped(RunStop::Abandoned(reason))),
    }
    Ok(())
}

/// What the yield watch watches, and whose statement it cancels.
#[derive(Clone, Copy, Debug)]
struct Watch {
    /// The table.
    relid: u32,
    /// The run's own sessions, which the watch never yields to.
    own: [i32; 2],
    /// The main session, whose statement the watch cancels.
    main_pid: i32,
}

/// Clean up a run that stopped before its swap committed on the guard's
/// transaction, within the shutdown budget when the run was cancelled.
async fn clean_up_on_guard(guard: Transaction<'_, Postgres>, main: &mut PgConnection, plan: CopyPlan<'_>, stop: RunStop) -> Stopped {
    let deadline = Deadline::now() + SHUTDOWN_CLEANUP_BUDGET;
    let cleanup = cleanup::clean_up_on_guard(guard, main, plan.watch.main_pid, plan.subject, plan.table);
    let cleanup = match stop {
        RunStop::Cancelled => time::timeout_at(deadline, cleanup).await.ok().flatten(),
        RunStop::Abandoned(_) | RunStop::Yielded | RunStop::Failed(_) => cleanup.await,
    };
    Stopped { stop, cleanup, deadline }
}

/// What the copy, the builds and the replay run with.
#[derive(Clone, Copy)]
pub(super) struct CopyPlan<'p> {
    /// The table being repacked.
    subject: RepackSubject<'p>,
    /// The statements the extension generated for it.
    script: &'p RepackTableScript,
    /// Its qualified name.
    table: &'p TableName,
    /// Each long statement's `statement_timeout`.
    budget: Duration,
    /// What the yield watch watches.
    watch: Watch,
    /// The guard session's backend, left out of the old transactions.
    guard_pid: i32,
}

impl<'p> CopyPlan<'p> {
    /// The plan for repacking the subject's `table` from `script`, each long
    /// statement allowed `budget`, through the main session `main_pid` and
    /// the guard `guard_pid`.
    pub(super) const fn new(
        subject: RepackSubject<'p>,
        script: &'p RepackTableScript,
        table: &'p TableName,
        budget: Duration,
        main_pid: i32,
        guard_pid: i32,
    ) -> CopyPlan<'p> {
        CopyPlan {
            subject,
            script,
            table,
            budget,
            watch: Watch {
                relid: subject.relid,
                own: [main_pid, guard_pid],
                main_pid,
            },
            guard_pid,
        }
    }
}

/// Copy, build and replay, each phase recorded and timed and run under the
/// yield watch on the guard.
async fn copy_build_replay(
    platform: &mut PgConnection,
    guard: &mut Transaction<'_, Postgres>,
    main: &mut PgConnection,
    plan: CopyPlan<'_>,
    cancellation: &CancellationToken,
) -> Result<(), RunStop> {
    let CopyPlan {
        subject,
        script,
        table,
        budget,
        watch,
        guard_pid,
    } = plan;
    begin_phase(platform, subject, RepackPhase::Copy).await?;
    let started = Instant::now();
    let old = match watched(guard, watch, cancellation, copy::copy_rows(main, guard_pid, script, table, budget)).await? {
        CopyOutcome::Copied(old) => old,
        CopyOutcome::Abandoned(reason) => return Err(RunStop::Abandoned(reason)),
    };
    record_repack_phase(RepackPhase::Copy.label(), started.elapsed());
    begin_phase(platform, subject, RepackPhase::IndexBuild).await?;
    let started = Instant::now();
    watched(guard, watch, cancellation, copy::build_indexes(main, script, budget)).await?;
    record_repack_phase(RepackPhase::IndexBuild.label(), started.elapsed());
    begin_phase(platform, subject, RepackPhase::Replay).await?;
    let started = Instant::now();
    if let PhaseEnd::Abandoned(reason) = watched(guard, watch, cancellation, swap::replay_until_converged(main, script, &old)).await? {
        return Err(RunStop::Abandoned(reason));
    }
    record_repack_phase(RepackPhase::Replay.label(), started.elapsed());
    Ok(())
}

/// Record the run reaching `phase` and log it.
async fn begin_phase(platform: &mut PgConnection, subject: RepackSubject<'_>, phase: RepackPhase) -> Result<(), MaintenanceError> {
    repack_run::record_phase(platform, subject.tenant, subject.row, phase).await?;
    info!(tenant = subject.tenant.log_name(), database = %subject.tenant.database, branch = ?subject.row.branch(), target = subject.heap.relation_name(), phase = %phase, decision = "repack-phase", "PostgreSQL repack reached a phase");
    Ok(())
}

/// Run `phase` while the guard watches the table for waiting sessions and
/// cancellation may stop it.
async fn watched<T, F>(guard: &mut Transaction<'_, Postgres>, watch: Watch, cancellation: &CancellationToken, phase: F) -> Result<T, RunStop>
where
    F: Future<Output = Result<T, MaintenanceError>>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(RunStop::Cancelled),
        watched = yield_watch::watch(guard, watch.relid, watch.own, watch.main_pid) => {
            watched?;
            Err(RunStop::Yielded)
        }
        result = phase => Ok(result?),
    }
}

/// End a run that stopped after its setup: keep the guard's cleanup when it
/// cleaned up, and otherwise clean up on the main session after a run that
/// gave up on its own, or on a fresh session, within the shutdown budget for
/// a cancelled run.
pub(super) async fn finish_stopped(
    platform: &mut PgConnection,
    mut sessions: RunSessions,
    subject: RepackSubject<'_>,
    Stopped { stop, cleanup, deadline }: Stopped,
) -> Result<RepackOutcome, MaintenanceError> {
    let cleanup = match (cleanup, &stop) {
        (Some(CleanupOutcome::Cleaned), _) => cleanup::end_after_guard_cleanup(platform, &mut sessions.main, subject).await?,
        (Some(CleanupOutcome::HeldElsewhere | CleanupOutcome::LockWait | CleanupOutcome::Vanished) | None, RunStop::Abandoned(_)) => {
            cleanup::clean_up_on_main(platform, &mut sessions.main, subject).await?
        }
        (Some(CleanupOutcome::HeldElsewhere | CleanupOutcome::LockWait | CleanupOutcome::Vanished) | None, RunStop::Cancelled) => {
            let cleaned = time::timeout_at(deadline, cleanup::clean_up_on_fresh_session(platform, sessions, subject)).await;
            return Ok(RepackOutcome::Cancelled {
                cleanup: cleaned.ok().transpose()?,
            });
        }
        (Some(CleanupOutcome::HeldElsewhere | CleanupOutcome::LockWait | CleanupOutcome::Vanished) | None, RunStop::Yielded | RunStop::Failed(_)) => {
            cleanup::clean_up_on_fresh_session(platform, sessions, subject).await?
        }
    };
    Ok(match stop {
        RunStop::Abandoned(reason) => RepackOutcome::Abandoned {
            reason,
            cleanup: Some(cleanup),
        },
        RunStop::Yielded => RepackOutcome::Abandoned {
            reason: RepackAbandonment::Yielded,
            cleanup: Some(cleanup),
        },
        RunStop::Cancelled => RepackOutcome::Cancelled { cleanup: Some(cleanup) },
        RunStop::Failed(error) => RepackOutcome::Failed { error, cleanup: Some(cleanup) },
    })
}

/// Run `phase` unless cancellation stops it first.
async fn cancellable<T, F>(cancellation: &CancellationToken, phase: F) -> Result<T, RunStop>
where
    F: Future<Output = Result<T, MaintenanceError>>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(RunStop::Cancelled),
        result = phase => Ok(result?),
    }
}
