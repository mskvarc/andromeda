//! Pacing a resident tenant's timer-driven background work by what is due.
//!
//! A resident tenant runs a family of background tasks against its own
//! database: the `expiresAt` sweeps, Snapshot preparation recovery, the
//! durable drains (notification deliveries, side effects, temporal append
//! intents, Snapshot status notifications), the clause-5.8.6 periodic
//! notification sweeps, the clause-5.13.1 cached-`@context` invalidation and
//! the table-size gauges. Each runs on its own clock -- a fixed cadence, or a
//! drain's fallback timer backing off to a ceiling -- and every tick touches
//! the tenant's pool. Through a transaction pooler, a touched pool keeps its
//! server connection for the pooler's `server_idle_timeout` after the tick, and
//! a server held idle for one database is not handed to another until that
//! timeout closes it. A handful of independent clocks per tenant therefore keep
//! a server attached to every cached tenant, a few hundred cached tenants pin
//! the pooler's global cap, and a tenant loaded next queues behind them until
//! its acquire deadline sheds it -- the opposite of what
//! `docs/pgbouncer-deployment.md` promises: that the fleet's backend count
//! tracks active tenants, not cached ones.
//!
//! The pacer lets a timer tick run only when something is due. The tenant's
//! earliest outstanding obligation is read once -- the same read the wake loop
//! uses to decide when a dormant tenant is next owed work, which covers every
//! durable-queue item and its retry, every preparing Snapshot, every
//! `expiresAt` and every periodic notification -- and that one verdict decides
//! for every clock of the tenant until it expires or the tenant is used again:
//! any request may have created an obligation the read did not see, so a use
//! voids the verdict and the next tick reads afresh. A tick that finds
//! something due runs; a tick that finds nothing rests until the obligation
//! falls due, and a drain resting on its fallback timer sleeps exactly until
//! then instead of polling toward it. A tenant with nothing outstanding issues
//! one read per [`QUIET_CEILING_CADENCES`] cadences and nothing else.
//!
//! Work a producer asks for is never paced: a drain pass started by a
//! commit's wake, or continuing a pass that found more work, always runs.
//! Only the timers are paced, and every obligation they serve is in the read:
//!
//! | Timer | Obligation in the read |
//! |---|---|
//! | entity, subscription (entity and context source), registration, Snapshot and Entity Map `expiresAt` sweeps | the earliest `expiresAt` of each |
//! | Snapshot preparation recovery | a preparing Snapshot's lease expiry |
//! | notification-delivery, side-effect, temporal-intent and Snapshot-status drains | each queue's earliest claimable item, retries included |
//! | entity and context source periodic notifications | each active periodic subscription's next grid point |
//! | cached-`@context` invalidation | the earliest expiration of a `"Cached"` `@context` |
//!
//! The table-size gauges serve no obligation: they report sizes, which move
//! only when something writes. They sample when the tenant has been used or a
//! background pass has run since their last sample, and rest otherwise.

use andromeda_postgresql_context_store::{error::ContextStoreError, pending_work::earliest_obligation};
use andromeda_task_runtime::{control::DrainCause, drain_task::DrainPassOutcome, failure::TaskFailure};
use andromeda_tenancy::state_factory::RuntimeActivity;
use async_trait::async_trait;
use sqlx::PgPool;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{sync::Mutex, time::Instant};

/// How many cadences a tenant with nothing outstanding rests before it reads
/// its obligations again.
///
/// A safety bound only: every obligation a timer serves is visible to the read,
/// and every write that creates one reaches the tenant through a request that
/// voids the verdict. The bound caps what an obligation created by any other
/// path could wait, and it is the cadence of the cached-`@context`
/// invalidation of a tenant nobody uses.
pub(crate) const QUIET_CEILING_CADENCES: u32 = 16;

/// Where a tenant's earliest outstanding obligation is read from.
#[async_trait]
pub(crate) trait ObligationSource: Send + Sync + 'static {
    /// The earliest instant any obligation falls due, or `None` when nothing is
    /// outstanding.
    async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError>;
}

/// The tenant's own database, read over its serving pool.
pub(crate) struct TenantObligations {
    /// The tenant's serving pool.
    pool: PgPool,
}

impl TenantObligations {
    /// Read obligations over `pool`.
    pub(crate) fn new(pool: &PgPool) -> TenantObligations {
        TenantObligations {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
        }
    }
}

#[async_trait]
impl ObligationSource for TenantObligations {
    async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
        earliest_obligation(&self.pool).await
    }
}

/// Whether a timer tick does its work or rests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassAdmission {
    /// Run the pass.
    Sweep,
    /// Skip this pass without touching the database; nothing is due before
    /// `until`.
    Rest {
        /// When the tenant's earliest obligation falls due, or its next
        /// obligation read is taken, whichever is sooner.
        until: Instant,
    },
}

/// What the last obligation read decided, for every timer of the tenant.
#[derive(Debug, Clone, Copy)]
enum Verdict {
    /// No read yet, or the last one failed.
    Unread,
    /// An obligation is due: every tick runs until `until`, then reads again.
    Sweeping {
        /// How many changes the tenant had seen when the read was taken; a
        /// later change voids the verdict.
        seen_changes: u64,
        /// The end of the window in which the other timers' ticks run without
        /// reading again.
        until: Instant,
    },
    /// Nothing is due before `until`.
    Resting {
        /// How many changes the tenant had seen when the read was taken; a
        /// later change voids the verdict.
        seen_changes: u64,
        /// When the tenant next reads its obligations.
        until: Instant,
    },
}

/// Decides, tick by tick, whether a resident tenant's timer-driven background
/// work runs.
pub(crate) struct BackgroundPacer {
    /// The sweeps' cadence: the width of a due verdict's window is half of it,
    /// and the read ceiling is [`QUIET_CEILING_CADENCES`] of it.
    cadence: Duration,
    /// The longest a tenant with nothing outstanding rests between reads.
    ceiling: Duration,
    /// Where the earliest obligation is read from.
    obligations: Arc<dyn ObligationSource>,
    /// How many times the tenant may have changed since the pacer started: each
    /// request reaching for or letting go of the runtime, and each background
    /// pass that ran to completion. A verdict remembers the count it was read
    /// under, so any change since -- however close in time -- voids it, and the
    /// table-size gauges sample only after a change.
    changes: AtomicU64,
    /// The last decision, shared by every timer of the tenant so one read
    /// serves them all.
    verdict: Mutex<Verdict>,
}

impl BackgroundPacer {
    /// A pacer whose due window and read ceiling are measured in `cadence`,
    /// reading obligations from `obligations`.
    pub(crate) fn new(cadence: Duration, obligations: Arc<dyn ObligationSource>) -> BackgroundPacer {
        BackgroundPacer {
            cadence,
            ceiling: cadence.saturating_mul(QUIET_CEILING_CADENCES),
            obligations,
            changes: AtomicU64::new(0),
            verdict: Mutex::new(Verdict::Unread),
        }
    }

    /// Note that the tenant may have changed, voiding the current verdict.
    fn record_change(&self) {
        self.changes.fetch_add(1, Ordering::AcqRel);
    }

    /// Run one sample of an observation of the tenant's tables if the tenant may
    /// have changed since the observation last ran, and do nothing otherwise.
    ///
    /// `observed` is the change count the observation last ran under, owned by
    /// the observation. The first sample always runs. A sample is not itself a
    /// change: it only reads.
    pub(crate) async fn run_when_written(&self, observed: &AtomicU64, sample: impl Future<Output = Result<(), TaskFailure>>) -> Result<(), TaskFailure> {
        // One more than the count, so a freshly created observation (holding
        // zero) samples once even before anything changes.
        let changes = self.changes.load(Ordering::Acquire) + 1;
        if observed.swap(changes, Ordering::AcqRel) == changes {
            return Ok(());
        }
        sample.await
    }

    /// Run `pass` and record the change it may have made once it has finished,
    /// so a verdict read while it ran -- which cannot have seen what it wrote --
    /// does not outlive it.
    async fn run_changing<T>(&self, pass: impl Future<Output = T>) -> T {
        let outcome = pass.await;
        self.record_change();
        outcome
    }

    /// Whether this timer tick does its work.
    ///
    /// A read that fails falls back to sweeping: the pass itself is the
    /// authority on what is due, so an unreadable obligation never delays one.
    pub(crate) async fn admit(&self) -> PassAdmission {
        let now = Instant::now();
        let changes = self.changes.load(Ordering::Acquire);
        let mut verdict = self.verdict.lock().await;
        match *verdict {
            Verdict::Sweeping { seen_changes, until } if seen_changes == changes && now < until => return PassAdmission::Sweep,
            Verdict::Resting { seen_changes, until } if seen_changes == changes && now < until => return PassAdmission::Rest { until },
            Verdict::Unread | Verdict::Sweeping { .. } | Verdict::Resting { .. } => {}
        }
        match self.obligations.earliest_due().await {
            Ok(due) => {
                let (read, admission) = self.verdict_for(now, changes, due);
                *verdict = read;
                admission
            }
            Err(error) => {
                tracing::warn!(%error, event = "background_pacer_obligation_read_failed");
                *verdict = Verdict::Unread;
                PassAdmission::Sweep
            }
        }
    }

    /// Run one tick of a scheduled sweep if it is admitted, and do nothing
    /// otherwise.
    ///
    /// `sweep` is a future, so a resting tick never starts it and never touches
    /// the database; the sweep's own schedule brings the next tick.
    pub(crate) async fn run(&self, sweep: impl Future<Output = Result<(), TaskFailure>>) -> Result<(), TaskFailure> {
        match self.admit().await {
            PassAdmission::Sweep => self.run_changing(sweep).await,
            PassAdmission::Rest { .. } => Ok(()),
        }
    }

    /// Run one durable drain pass.
    ///
    /// A pass a producer's wake started, or that continues one that found more
    /// work, always runs. A pass the fallback timer started runs only when
    /// something is due; otherwise it reports the drain idle until the rest
    /// ends, so the drain sleeps exactly until the tenant's earliest obligation
    /// falls due rather than polling toward it.
    pub(crate) async fn run_drain_pass(
        &self,
        cause: DrainCause,
        pass: impl Future<Output = Result<DrainPassOutcome, TaskFailure>>,
    ) -> Result<DrainPassOutcome, TaskFailure> {
        match cause {
            DrainCause::Wake | DrainCause::Continuation => self.run_changing(pass).await,
            DrainCause::Fallback => match self.admit().await {
                PassAdmission::Sweep => self.run_changing(pass).await,
                PassAdmission::Rest { until } => Ok(DrainPassOutcome::IdleUntil(SystemTime::now() + until.saturating_duration_since(Instant::now()))),
            },
        }
    }

    /// The decision a read taken at `now`, after `seen_changes` changes, that
    /// found `due` leads to.
    fn verdict_for(&self, now: Instant, seen_changes: u64, due: Option<SystemTime>) -> (Verdict, PassAdmission) {
        // An instant already past has no duration until it; it is due now.
        match due.map(|due| due.duration_since(SystemTime::now()).unwrap_or(Duration::ZERO)) {
            Some(wait) if wait.is_zero() => (
                Verdict::Sweeping {
                    seen_changes,
                    until: now + self.cadence / 2,
                },
                PassAdmission::Sweep,
            ),
            Some(wait) => {
                let until = now + wait.min(self.ceiling);
                (Verdict::Resting { seen_changes, until }, PassAdmission::Rest { until })
            }
            None => {
                let until = now + self.ceiling;
                (Verdict::Resting { seen_changes, until }, PassAdmission::Rest { until })
            }
        }
    }
}

impl RuntimeActivity for BackgroundPacer {
    /// A request reached for or let go of the runtime: what it committed may be
    /// an obligation the current verdict never saw.
    fn record_activity(&self) {
        self.record_change();
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::background_pacer::{BackgroundPacer, ObligationSource, PassAdmission, QUIET_CEILING_CADENCES};
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use andromeda_task_runtime::{control::DrainCause, drain_task::DrainPassOutcome};
    use andromeda_tenancy::state_factory::RuntimeActivity;
    use async_trait::async_trait;
    use std::{
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime},
    };
    use tokio::time::advance;

    const CADENCE: Duration = Duration::from_secs(30);

    /// An obligation source answering a scripted due offset and counting reads.
    struct ScriptedObligations {
        /// The next answer: `None` nothing due, `Some(offset)` due that long
        /// from the read, `Some(ZERO)` due now.
        due_in: Mutex<Option<Duration>>,
        /// Whether reads fail.
        failing: bool,
        /// Reads taken.
        reads: AtomicUsize,
    }

    impl ScriptedObligations {
        fn answering(due_in: Option<Duration>) -> Arc<ScriptedObligations> {
            Arc::new(ScriptedObligations {
                due_in: Mutex::new(due_in),
                failing: false,
                reads: AtomicUsize::new(0),
            })
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::Relaxed)
        }

        fn answer(&self, due_in: Option<Duration>) {
            *self.due_in.lock().expect("script lock") = due_in;
        }
    }

    #[async_trait]
    impl ObligationSource for ScriptedObligations {
        async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.failing {
                return Err(ContextStoreError::Internal("unreadable".to_string()));
            }
            let due_in = *self.due_in.lock().expect("script lock");
            Ok(due_in.map(|offset| SystemTime::now() + offset))
        }
    }

    fn pacer_over(obligations: &Arc<ScriptedObligations>) -> BackgroundPacer {
        BackgroundPacer::new(CADENCE, Arc::clone(obligations) as Arc<dyn ObligationSource>)
    }

    #[tokio::test(start_paused = true)]
    async fn a_tenant_with_nothing_outstanding_rests_every_timer_on_one_read_until_the_ceiling() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        for _ in 0..QUIET_CEILING_CADENCES {
            for _ in 0..6 {
                assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }));
            }
            advance(CADENCE).await;
        }
        pretty_assertions::assert_eq!(obligations.reads(), 1, "one read serves every timer up to the ceiling");
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }));
        pretty_assertions::assert_eq!(obligations.reads(), 2, "the ceiling triggers the next read");
    }

    #[tokio::test(start_paused = true)]
    async fn a_tenant_rests_until_its_earliest_obligation_then_sweeps() {
        let obligations = ScriptedObligations::answering(Some(CADENCE * 3));
        let pacer = pacer_over(&obligations);
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }));
        advance(CADENCE * 2).await;
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }));
        pretty_assertions::assert_eq!(obligations.reads(), 1);
        obligations.answer(Some(Duration::ZERO));
        advance(CADENCE).await;
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep, "the obligation fell due");
    }

    /// Every timer of the tenant ticks on its own clock; one read decides for
    /// all of them rather than each reading for itself.
    #[tokio::test(start_paused = true)]
    async fn one_read_admits_every_timer_while_an_obligation_is_due() {
        let obligations = ScriptedObligations::answering(Some(Duration::ZERO));
        let pacer = pacer_over(&obligations);
        for _ in 0..6 {
            pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        }
        pretty_assertions::assert_eq!(obligations.reads(), 1);
    }

    /// A request can create an obligation the last read never saw, so a use
    /// voids the verdict and the next tick reads afresh.
    #[tokio::test(start_paused = true)]
    async fn a_use_voids_the_verdict_so_the_next_tick_reads_afresh() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }));
        obligations.answer(Some(Duration::ZERO));
        pacer.record_activity();
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 2);
    }

    /// A tick taken while a request holds the runtime reads before the request
    /// commits; the request's release must void that reading, so the next tick
    /// sees what the request committed (clause 5.8.6: a periodic subscription
    /// the request created is notified when its interval is reached, not at
    /// the read ceiling).
    #[tokio::test(start_paused = true)]
    async fn a_verdict_read_during_a_request_does_not_survive_the_request() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        pacer.record_activity(); // the request takes its lease
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }), "the tick reads before the commit");
        obligations.answer(Some(Duration::ZERO)); // the request commits a due obligation
        pacer.record_activity(); // the request lets its lease go
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep, "the next tick sees the commit");
        pretty_assertions::assert_eq!(obligations.reads(), 2);
    }

    /// A pass may itself write an obligation -- a delivery that failed and
    /// scheduled its retry -- that a verdict read while it ran never saw.
    #[tokio::test(start_paused = true)]
    async fn a_verdict_read_while_a_pass_ran_does_not_survive_the_pass() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        let outcome = pacer
            .run_drain_pass(DrainCause::Wake, async {
                assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }), "another timer reads mid-pass");
                obligations.answer(Some(Duration::ZERO)); // the pass schedules a retry
                Ok(DrainPassOutcome::Idle)
            })
            .await;
        assert!(matches!(outcome, Ok(DrainPassOutcome::Idle)), "{outcome:?}");
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep, "the retry is seen after the pass");
    }

    /// Work a producer asked for is never paced, even while the timers rest.
    #[tokio::test(start_paused = true)]
    async fn a_woken_or_continuing_drain_pass_runs_without_reading_obligations() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        for cause in [DrainCause::Wake, DrainCause::Continuation] {
            let outcome = pacer.run_drain_pass(cause, async { Ok(DrainPassOutcome::WorkMayRemain) }).await;
            assert!(matches!(outcome, Ok(DrainPassOutcome::WorkMayRemain)), "{cause:?}: {outcome:?}");
        }
        pretty_assertions::assert_eq!(obligations.reads(), 0);
    }

    /// A resting drain sleeps exactly until the obligation falls due instead
    /// of polling toward it, and its pass never starts.
    #[tokio::test(start_paused = true)]
    async fn a_resting_fallback_pass_sleeps_until_the_earliest_obligation_without_running() {
        let obligations = ScriptedObligations::answering(Some(CADENCE * 3));
        let pacer = pacer_over(&obligations);
        let ran = AtomicUsize::new(0);
        let before = SystemTime::now();
        let outcome = pacer
            .run_drain_pass(DrainCause::Fallback, async {
                ran.fetch_add(1, Ordering::Relaxed);
                Ok(DrainPassOutcome::Idle)
            })
            .await
            .expect("a resting pass cannot fail");
        let after = SystemTime::now();
        pretty_assertions::assert_eq!(ran.load(Ordering::Relaxed), 0, "a resting pass never starts");
        match outcome {
            DrainPassOutcome::IdleUntil(resume) => assert!(
                resume >= before + CADENCE * 3 - Duration::from_secs(1) && resume <= after + CADENCE * 3 + Duration::from_secs(1),
                "the drain resumes when the obligation falls due"
            ),
            DrainPassOutcome::Idle | DrainPassOutcome::WorkMayRemain => panic!("a resting drain reports when it resumes, got {outcome:?}"),
        }
    }

    /// Sizes move only when something writes, so a gauge samples once, then
    /// again only after a use or a background pass.
    #[tokio::test(start_paused = true)]
    async fn an_observation_samples_only_after_something_may_have_written() {
        let obligations = ScriptedObligations::answering(Some(Duration::ZERO));
        let pacer = pacer_over(&obligations);
        let observed = AtomicU64::new(0);
        let samples = AtomicUsize::new(0);
        let sample = || async {
            samples.fetch_add(1, Ordering::Relaxed);
            Ok(())
        };
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pretty_assertions::assert_eq!(samples.load(Ordering::Relaxed), 1, "nothing wrote since the first sample");
        pacer.record_activity();
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pretty_assertions::assert_eq!(samples.load(Ordering::Relaxed), 2, "a use may have written");
        pacer.run(async { Ok(()) }).await.expect("an admitted sweep");
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pretty_assertions::assert_eq!(samples.load(Ordering::Relaxed), 3, "a background pass may have written");
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pretty_assertions::assert_eq!(samples.load(Ordering::Relaxed), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unreadable_obligation_falls_back_to_sweeping() {
        let obligations = Arc::new(ScriptedObligations {
            due_in: Mutex::new(None),
            failing: true,
            reads: AtomicUsize::new(0),
        });
        let pacer = pacer_over(&obligations);
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 2, "a failed read is retried on the next tick");
    }
}
