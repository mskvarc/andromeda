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
//! its acquire deadline sheds it -- the opposite of the intent that the fleet's backend count
//! tracks active tenants, not cached ones.
//!
//! The pacer lets a timer tick of a tenant nobody is using run only when
//! something is due. The tenant's earliest outstanding obligation is read once
//! -- the same read the wake loop uses to decide when a dormant tenant is next
//! owed work, which covers every durable-queue item and its retry, every
//! preparing Snapshot, every `expiresAt` and every periodic notification --
//! and that one verdict decides for every clock of the tenant until it expires
//! or the tenant changes: any request may have created an obligation the read
//! did not see, so a use voids the verdict. A tick that finds something due
//! runs; a tick that finds nothing rests until the obligation falls due, and a
//! drain resting on its fallback timer sleeps exactly until then instead of
//! polling toward it. A tenant with nothing outstanding issues one read per
//! [`QUIET_CEILING_CADENCES`] cadences and nothing else.
//!
//! A tenant a request used within the last cadence already holds a
//! connection, so its ticks run without reading: a read would cost what it
//! saves. The first tick after a cadence without a use reads afresh.
//!
//! The read is single-flight. Ticks that need a read while one is in flight
//! wait for it and share its verdict, so the clocks of a tenant never queue to
//! read one after another, and no tick holds the verdict across the read: a
//! tick the standing verdict or a recent use decides is never delayed by a
//! slow read.
//!
//! Work a producer asks for is never paced: a drain pass started by a
//! commit's wake, or continuing a pass that found more work, always runs. Nor
//! is a pass that ends the sleep its own predecessor computed: a periodic
//! sweep that reported its next grid point is known to be due when that point
//! comes. Only the timers are paced, and every obligation they serve is in the
//! read:
//!
//! | Timer | Obligation in the read |
//! |---|---|
//! | entity, subscription (entity and context source), registration, Snapshot and Entity Map `expiresAt` sweeps | the earliest `expiresAt` of each |
//! | Snapshot preparation recovery | a preparing Snapshot's lease expiry |
//! | notification-delivery, side-effect, temporal-intent and Snapshot-status drains | each queue's earliest claimable item, retries included |
//! | entity and context source periodic notifications | each active periodic subscription's next grid point |
//! | cached-`@context` invalidation | the earliest expiration of a `"Cached"` `@context` |
//!
//! The table-size gauges and the notification-delivery cleanup serve no
//! obligation: the gauges report sizes, and the cleanup reaps finished queue
//! rows, and both move only when something writes. They run when the tenant has
//! been used or a background pass has run since they last ran, and rest
//! otherwise.

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
///
/// Each read carries the sequence number it finished under. A tick that was
/// waiting when a read finished shares that read's verdict, whatever it is,
/// when the read saw every change the tick knew of.
#[derive(Debug, Clone, Copy)]
enum Verdict {
    /// No read yet.
    Unread,
    /// The read failed: the ticks waiting for it sweep, and the next tick
    /// reads again.
    Failed {
        /// The sequence number of the read.
        read: u64,
        /// How many changes the tenant had seen when the read was taken.
        seen_changes: u64,
    },
    /// An obligation is due: every tick runs until `until`, then reads again.
    Sweeping {
        /// The sequence number of the read.
        read: u64,
        /// How many changes the tenant had seen when the read was taken; a
        /// later change voids the verdict.
        seen_changes: u64,
        /// The end of the window in which the other timers' ticks run without
        /// reading again.
        until: Instant,
    },
    /// Nothing is due before `until`.
    Resting {
        /// The sequence number of the read.
        read: u64,
        /// How many changes the tenant had seen when the read was taken; a
        /// later change voids the verdict.
        seen_changes: u64,
        /// When the tenant next reads its obligations.
        until: Instant,
    },
}

impl Verdict {
    /// The admission this verdict gives a tick that arrived at `now`, after
    /// `changes` changes and `reads_finished` finished reads: a verdict still
    /// in force, or one a read that saw the tick's changes and finished after
    /// the tick arrived produced. `None` when the tick has to read.
    fn admission_for(self, now: Instant, changes: u64, reads_finished: u64) -> Option<PassAdmission> {
        let awaited = |read: u64, seen_changes: u64| read > reads_finished && seen_changes >= changes;
        let standing = |seen_changes: u64, until: Instant| seen_changes == changes && now < until;
        match self {
            Verdict::Sweeping { read, seen_changes, until } if awaited(read, seen_changes) || standing(seen_changes, until) => Some(PassAdmission::Sweep),
            Verdict::Resting { read, seen_changes, until } if awaited(read, seen_changes) || standing(seen_changes, until) => {
                Some(PassAdmission::Rest { until })
            }
            Verdict::Failed { read, seen_changes } if awaited(read, seen_changes) => Some(PassAdmission::Sweep),
            Verdict::Unread | Verdict::Failed { .. } | Verdict::Sweeping { .. } | Verdict::Resting { .. } => None,
        }
    }
}

/// Decides, tick by tick, whether a resident tenant's timer-driven background
/// work runs.
pub(crate) struct BackgroundPacer {
    /// The sweeps' cadence: how recent a use lets a tick run without reading,
    /// the width of a due verdict's window is half of it, and the read ceiling
    /// is [`QUIET_CEILING_CADENCES`] of it.
    cadence: Duration,
    /// The longest a tenant with nothing outstanding rests between reads.
    ceiling: Duration,
    /// Where the earliest obligation is read from.
    obligations: Arc<dyn ObligationSource>,
    /// The instant [`BackgroundPacer::last_used`] is measured from.
    origin: Instant,
    /// When a request last reached for or let go of the runtime, as
    /// nanoseconds after `origin` plus one; zero before the first use.
    last_used: AtomicU64,
    /// How many times the tenant may have changed since the pacer started: each
    /// request reaching for or letting go of the runtime, and each background
    /// pass that ran to completion. A verdict remembers the count it was read
    /// under, so any change since -- however close in time -- voids it, and the
    /// table-size gauges sample only after a change.
    changes: AtomicU64,
    /// How many obligation reads have finished; the latest is the sequence
    /// number of the standing verdict.
    reads_finished: AtomicU64,
    /// The last decision, shared by every timer of the tenant so one read
    /// serves them all. Held only to read or replace it, never across a read.
    verdict: Mutex<Verdict>,
    /// Held by the one tick reading obligations; the others needing a read
    /// wait on it and then share that read's verdict.
    read_in_flight: Mutex<()>,
}

impl BackgroundPacer {
    /// A pacer whose recent-use window, due window and read ceiling are
    /// measured in `cadence`, reading obligations from `obligations`.
    pub(crate) fn new(cadence: Duration, obligations: Arc<dyn ObligationSource>) -> BackgroundPacer {
        BackgroundPacer {
            cadence,
            ceiling: cadence.saturating_mul(QUIET_CEILING_CADENCES),
            obligations,
            origin: Instant::now(),
            last_used: AtomicU64::new(0),
            changes: AtomicU64::new(0),
            reads_finished: AtomicU64::new(0),
            verdict: Mutex::new(Verdict::Unread),
            read_in_flight: Mutex::new(()),
        }
    }

    /// Note that the tenant may have changed, voiding the current verdict.
    fn record_change(&self) {
        self.changes.fetch_add(1, Ordering::SeqCst);
    }

    /// Whether a request reached for or let go of the runtime within the last
    /// cadence before `now`.
    fn used_within_cadence(&self, now: Instant) -> bool {
        match self.last_used.load(Ordering::Acquire) {
            0 => false,
            stamp => now.saturating_duration_since(self.origin + Duration::from_nanos(stamp - 1)) < self.cadence,
        }
    }

    /// Run one pass of work only a write can create -- a sample of the tenant's
    /// table sizes, a reap of finished queue rows -- if the tenant may have
    /// changed since that work last ran, and do nothing otherwise.
    ///
    /// `observed` is the change count the work last ran under, owned by the
    /// work. The first pass always runs. A pass is not itself a change: it
    /// either only reads or only removes what no obligation depends on, so
    /// counting it would have it run again on every tick.
    pub(crate) async fn run_when_written<E>(
        &self,
        observed: &AtomicU64,
        sample: impl Future<Output = Result<(), TaskFailure<E>>>,
    ) -> Result<(), TaskFailure<E>> {
        // One more than the count, so a freshly created observation (holding
        // zero) samples once even before anything changes.
        let changes = self.changes.load(Ordering::SeqCst) + 1;
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
    /// A tenant used within the cadence sweeps without reading. Otherwise the
    /// standing verdict decides, and when it has lapsed the tick reads, or
    /// waits for the read already in flight and takes its verdict.
    pub(crate) async fn admit(&self) -> PassAdmission {
        let now = Instant::now();
        if self.used_within_cadence(now) {
            return PassAdmission::Sweep;
        }
        let reads_finished = self.reads_finished.load(Ordering::SeqCst);
        let changes = self.changes.load(Ordering::SeqCst);
        if let Some(admission) = self.verdict.lock().await.admission_for(now, changes, reads_finished) {
            return admission;
        }
        let _reading = self.read_in_flight.lock().await;
        if let Some(admission) = self.verdict.lock().await.admission_for(now, changes, reads_finished) {
            return admission;
        }
        self.read().await
    }

    /// Read the tenant's obligations and record the verdict.
    ///
    /// A read that fails falls back to sweeping: the pass itself is the
    /// authority on what is due, so an unreadable obligation never delays one.
    async fn read(&self) -> PassAdmission {
        let seen_changes = self.changes.load(Ordering::SeqCst);
        let now = Instant::now();
        let due = self.obligations.earliest_due().await;
        let mut verdict = self.verdict.lock().await;
        let read = self.reads_finished.fetch_add(1, Ordering::SeqCst) + 1;
        let (read_verdict, admission) = match due {
            Ok(due) => self.verdict_for(now, read, seen_changes, due),
            Err(error) => {
                tracing::warn!(%error, event = "background_pacer_obligation_read_failed");
                (Verdict::Failed { read, seen_changes }, PassAdmission::Sweep)
            }
        };
        *verdict = read_verdict;
        admission
    }

    /// Run one tick of a scheduled sweep if it is admitted, and do nothing
    /// otherwise.
    ///
    /// `sweep` is a future, so a resting tick never starts it and never touches
    /// the database; the sweep's own schedule brings the next tick.
    pub(crate) async fn run<E>(&self, sweep: impl Future<Output = Result<(), TaskFailure<E>>>) -> Result<(), TaskFailure<E>> {
        match self.admit().await {
            PassAdmission::Sweep => self.run_changing(sweep).await,
            PassAdmission::Rest { .. } => Ok(()),
        }
    }

    /// Run one durable drain pass.
    ///
    /// A pass a producer's wake started, one that continues a pass that found
    /// more work, and one that ends the sleep until the due instant its
    /// predecessor reported always run. A pass the fallback timer started runs
    /// only when admitted; otherwise it reports the drain rested until the rest
    /// ends, so the drain sleeps exactly until the tenant's earliest obligation
    /// falls due rather than polling toward it.
    pub(crate) async fn run_drain_pass<E>(
        &self,
        cause: DrainCause,
        pass: impl Future<Output = Result<DrainPassOutcome, TaskFailure<E>>>,
    ) -> Result<DrainPassOutcome, TaskFailure<E>> {
        match cause {
            DrainCause::Wake | DrainCause::Continuation | DrainCause::Due => self.run_changing(pass).await,
            DrainCause::Fallback => match self.admit().await {
                PassAdmission::Sweep => self.run_changing(pass).await,
                PassAdmission::Rest { until } => Ok(DrainPassOutcome::RestUntil(SystemTime::now() + until.saturating_duration_since(Instant::now()))),
            },
        }
    }

    /// The decision read number `read`, taken at `now` after `seen_changes`
    /// changes, that found `due` leads to.
    fn verdict_for(&self, now: Instant, read: u64, seen_changes: u64, due: Option<SystemTime>) -> (Verdict, PassAdmission) {
        // An instant already past has no duration until it; it is due now.
        match due.map(|due| due.duration_since(SystemTime::now()).unwrap_or(Duration::ZERO)) {
            Some(wait) if wait.is_zero() => (
                Verdict::Sweeping {
                    read,
                    seen_changes,
                    until: now + self.cadence / 2,
                },
                PassAdmission::Sweep,
            ),
            Some(wait) => {
                let until = now + wait.min(self.ceiling);
                (Verdict::Resting { read, seen_changes, until }, PassAdmission::Rest { until })
            }
            None => {
                let until = now + self.ceiling;
                (Verdict::Resting { read, seen_changes, until }, PassAdmission::Rest { until })
            }
        }
    }
}

impl RuntimeActivity for BackgroundPacer {
    /// A request reached for or let go of the runtime: the tenant holds a
    /// connection, and what it committed may be an obligation the current
    /// verdict never saw.
    fn record_activity(&self) {
        let since_origin = Instant::now().saturating_duration_since(self.origin);
        let stamp = u64::try_from(since_origin.as_nanos()).unwrap_or(u64::MAX - 1).saturating_add(1);
        self.last_used.fetch_max(stamp, Ordering::AcqRel);
        self.record_change();
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::background_pacer::{BackgroundPacer, ObligationSource, PassAdmission, QUIET_CEILING_CADENCES};
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use andromeda_task_runtime::{control::DrainCause, drain_task::DrainPassOutcome, failure::TaskFailure};
    use andromeda_tenancy::state_factory::RuntimeActivity;
    use async_trait::async_trait;
    use futures::future::join_all;
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime},
    };
    use tokio::{
        task::yield_now,
        time::{Instant, advance, sleep},
    };

    const CADENCE: Duration = Duration::from_secs(30);

    /// An obligation source answering a scripted due offset and counting reads.
    struct ScriptedObligations {
        /// The next answer: `None` nothing due, `Some(offset)` due that long
        /// from the read, `Some(ZERO)` due now.
        due_in: Mutex<Option<Duration>>,
        /// Whether reads fail.
        failing: bool,
        /// How long each read takes.
        latency: Duration,
        /// Reads taken.
        reads: AtomicUsize,
    }

    impl ScriptedObligations {
        fn answering(due_in: Option<Duration>) -> Arc<ScriptedObligations> {
            ScriptedObligations::answering_after(due_in, Duration::ZERO)
        }

        fn answering_after(due_in: Option<Duration>, latency: Duration) -> Arc<ScriptedObligations> {
            Arc::new(ScriptedObligations {
                due_in: Mutex::new(due_in),
                failing: false,
                latency,
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
            sleep(self.latency).await;
            if self.failing {
                return Err(ContextStoreError::Database(sqlx::Error::RowNotFound));
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
    /// voids the verdict: the tenant sweeps without reading while the use is
    /// recent, and the first tick a cadence after it reads afresh.
    #[tokio::test(start_paused = true)]
    async fn a_use_voids_the_verdict_so_the_first_tick_a_cadence_later_reads_afresh() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }));
        obligations.answer(Some(Duration::ZERO));
        pacer.record_activity();
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep, "a used tenant sweeps");
        pretty_assertions::assert_eq!(obligations.reads(), 1, "a used tenant does not read");
        advance(CADENCE).await;
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 2);
    }

    /// A tick taken while a long request holds the runtime reads before the
    /// request commits; the request's release must void that reading, so the
    /// first read after it sees what the request committed (clause 5.8.6: a
    /// periodic subscription the request created is notified when its
    /// interval is reached, not at the read ceiling).
    #[tokio::test(start_paused = true)]
    async fn a_verdict_read_during_a_request_does_not_survive_the_request() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        pacer.record_activity(); // the request takes its lease
        advance(CADENCE).await;
        assert!(matches!(pacer.admit().await, PassAdmission::Rest { .. }), "the tick reads before the commit");
        obligations.answer(Some(Duration::ZERO)); // the request commits a due obligation
        pacer.record_activity(); // the request lets its lease go
        advance(CADENCE).await;
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep, "the next read sees the commit");
        pretty_assertions::assert_eq!(obligations.reads(), 2);
    }

    /// A tenant a request just used holds a connection already, so no tick of
    /// any of its timers reads its obligations.
    #[tokio::test(start_paused = true)]
    async fn ten_concurrent_fallback_ticks_on_a_tenant_used_within_the_cadence_make_no_obligation_read() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = pacer_over(&obligations);
        pacer.record_activity();
        let outcomes =
            join_all((0..10).map(|_| pacer.run_drain_pass(DrainCause::Fallback, async { Ok::<_, TaskFailure<Infallible>>(DrainPassOutcome::Idle) }))).await;
        for outcome in outcomes {
            assert!(matches!(outcome, Ok(DrainPassOutcome::Idle)), "every tick runs its pass: {outcome:?}");
        }
        pretty_assertions::assert_eq!(obligations.reads(), 0);
    }

    /// The ticks of an idle tenant that need a read while one is in flight
    /// wait for it and take its verdict instead of reading in turn.
    #[tokio::test(start_paused = true)]
    async fn ten_concurrent_fallback_ticks_on_an_idle_tenant_make_one_read_and_share_its_verdict() {
        let obligations = ScriptedObligations::answering_after(Some(CADENCE * 3), Duration::from_secs(2));
        let pacer = pacer_over(&obligations);
        let admissions = join_all((0..10).map(|_| pacer.admit())).await;
        pretty_assertions::assert_eq!(obligations.reads(), 1);
        let first = admissions.first().copied().expect("ten admissions");
        assert!(matches!(first, PassAdmission::Rest { .. }), "{first:?}");
        for admission in admissions {
            pretty_assertions::assert_eq!(admission, first, "every tick takes the one read's verdict");
        }
    }

    /// A slow read holds back only the ticks that need it: a tick a recent use
    /// decides, and a periodic sweep's pass at the grid point its predecessor
    /// computed, run at once.
    #[tokio::test(start_paused = true)]
    async fn a_read_that_takes_two_seconds_delays_no_tick_that_does_not_need_it() {
        let obligations = ScriptedObligations::answering_after(None, Duration::from_secs(2));
        let pacer = Arc::new(pacer_over(&obligations));
        let reading = tokio::spawn({
            let pacer = Arc::clone(&pacer); // clone: the spawned tick owns its handle on the pacer.
            async move { pacer.admit().await }
        });
        yield_now().await;
        pretty_assertions::assert_eq!(obligations.reads(), 1, "a read is in flight");
        let started = Instant::now();
        let due = pacer
            .run_drain_pass(DrainCause::Due, async { Ok::<_, TaskFailure<Infallible>>(DrainPassOutcome::Idle) })
            .await;
        assert!(matches!(due, Ok(DrainPassOutcome::Idle)), "the due pass runs: {due:?}");
        pacer.record_activity();
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep, "a used tenant sweeps");
        pretty_assertions::assert_eq!(Instant::now(), started, "neither tick waited for the read");
        let read = reading.await.expect("the reading tick finishes");
        assert!(matches!(read, PassAdmission::Rest { .. }), "{read:?}");
        pretty_assertions::assert_eq!(obligations.reads(), 1);
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
                Ok::<_, TaskFailure<Infallible>>(DrainPassOutcome::Idle)
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
            let outcome = pacer
                .run_drain_pass(cause, async { Ok::<_, TaskFailure<Infallible>>(DrainPassOutcome::WorkMayRemain) })
                .await;
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
                Ok::<_, TaskFailure<Infallible>>(DrainPassOutcome::Idle)
            })
            .await
            .expect("a resting pass cannot fail");
        let after = SystemTime::now();
        pretty_assertions::assert_eq!(ran.load(Ordering::Relaxed), 0, "a resting pass never starts");
        match outcome {
            DrainPassOutcome::RestUntil(resume) => assert!(
                resume >= before + CADENCE * 3 - Duration::from_secs(1) && resume <= after + CADENCE * 3 + Duration::from_secs(1),
                "the drain resumes when the obligation falls due"
            ),
            DrainPassOutcome::Idle | DrainPassOutcome::WorkMayRemain | DrainPassOutcome::IdleUntil(_) => {
                panic!("a resting drain reports when it resumes, got {outcome:?}")
            }
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
            Ok::<(), TaskFailure<Infallible>>(())
        };
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pretty_assertions::assert_eq!(samples.load(Ordering::Relaxed), 1, "nothing wrote since the first sample");
        pacer.record_activity();
        pacer.run_when_written(&observed, sample()).await.expect("sample");
        pretty_assertions::assert_eq!(samples.load(Ordering::Relaxed), 2, "a use may have written");
        pacer.run(async { Ok::<(), TaskFailure<Infallible>>(()) }).await.expect("an admitted sweep");
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
            latency: Duration::ZERO,
            reads: AtomicUsize::new(0),
        });
        let pacer = pacer_over(&obligations);
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        pretty_assertions::assert_eq!(pacer.admit().await, PassAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 2, "a failed read is retried on the next tick");
    }
}
