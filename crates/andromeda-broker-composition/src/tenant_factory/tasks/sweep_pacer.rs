//! Pacing a resident tenant's obligation sweeps by whether it is in use.
//!
//! A resident tenant runs a family of sweeps -- every `expiresAt` sweep and the
//! Snapshot preparation recovery -- each on its own fixed cadence. Polling at
//! that cadence while nobody uses the tenant is what `docs/pgbouncer-deployment.md`
//! promises does not happen: a cached-but-idle tenant is to issue no periodic
//! SQL, so the pooler's `server_idle_timeout` reclaims its backends and the
//! fleet's backend count tracks active tenants rather than cached ones. Six
//! sweeps each firing every cadence keep every cached tenant's server
//! connection warm; past the pooler's global cap, a cold tenant then queues
//! behind hundreds of idle ones until its acquire deadline sheds it.
//!
//! The pacer keeps the fixed cadence exactly while the tenant is in use: every
//! request admitted to the runtime counts as use, and within one cadence of the
//! last use every sweep runs on its own tick as it always did. Once the tenant
//! goes quiet, the sweeps rest until the tenant's earliest obligation falls due
//! -- the same single read the wake loop uses to decide when a dormant tenant is
//! next owed work, which covers every `expiresAt`, every durable-queue item and
//! every preparing Snapshot -- and a quiet tenant with nothing outstanding
//! issues one such read per [`QUIET_CEILING_CADENCES`] cadences and nothing
//! else. Work that outlives the last request (an asynchronous batch still
//! draining, a Snapshot still preparing) is itself an outstanding obligation, so
//! the pacer keeps sweeping until it is gone.

use andromeda_postgresql_context_store::{error::ContextStoreError, pending_work::earliest_obligation};
use andromeda_task_runtime::failure::TaskFailure;
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

/// How many sweep cadences a quiet tenant with nothing outstanding rests
/// before it reads its obligations again.
///
/// A safety bound only: every obligation the sweeps serve is visible to the
/// read, and every write that creates one reaches the tenant through a request
/// that marks it in use. The bound caps what an obligation created by any
/// other path could wait.
pub(crate) const QUIET_CEILING_CADENCES: u32 = 16;

/// Where a quiet tenant's earliest outstanding obligation is read from.
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

/// Whether a sweep's tick does its work or rests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SweepAdmission {
    /// Run the sweep.
    Sweep,
    /// Skip this tick without touching the database.
    Rest,
}

/// What the last obligation read of a quiet tenant decided.
#[derive(Debug, Clone, Copy)]
enum QuietVerdict {
    /// No read since the tenant was last in use.
    Unread,
    /// An obligation is due: every sweep runs until `until`, then reads again.
    Sweeping {
        /// When the read was taken.
        read_at: Instant,
        /// The end of the window in which the other sweeps' ticks run without
        /// reading again.
        until: Instant,
    },
    /// Nothing is due before `until`.
    Resting {
        /// When the read was taken.
        read_at: Instant,
        /// When the tenant next reads its obligations.
        until: Instant,
    },
}

/// Decides, tick by tick, whether a resident tenant's sweeps do their work.
pub(crate) struct SweepPacer {
    /// The sweeps' cadence, and the width of the in-use window.
    cadence: Duration,
    /// The longest a quiet tenant rests between obligation reads.
    ceiling: Duration,
    /// Where the earliest obligation is read from.
    obligations: Arc<dyn ObligationSource>,
    /// The instant the pacer was created; activity is stored relative to it.
    origin: Instant,
    /// Milliseconds after `origin` of the last use.
    last_use_millis: AtomicU64,
    /// The last quiet-period decision, shared by every sweep of the tenant so
    /// one read serves them all.
    quiet: Mutex<QuietVerdict>,
}

impl SweepPacer {
    /// A pacer for sweeps running every `cadence`, reading quiet obligations
    /// from `obligations`. A new tenant counts as just used.
    pub(crate) fn new(cadence: Duration, obligations: Arc<dyn ObligationSource>) -> SweepPacer {
        SweepPacer {
            cadence,
            ceiling: cadence.saturating_mul(QUIET_CEILING_CADENCES),
            obligations,
            origin: Instant::now(),
            last_use_millis: AtomicU64::new(0),
            quiet: Mutex::new(QuietVerdict::Unread),
        }
    }

    /// Record that the tenant was just used.
    pub(crate) fn record_activity(&self) {
        self.last_use_millis.store(millis_since(self.origin, Instant::now()), Ordering::Relaxed);
    }

    /// When the tenant was last used.
    fn last_use(&self) -> Instant {
        self.origin + Duration::from_millis(self.last_use_millis.load(Ordering::Relaxed))
    }

    /// Whether this tick of a sweep does its work.
    ///
    /// A read that fails falls back to sweeping: the sweep itself is the
    /// authority on what is due, so an unreadable obligation never delays one.
    pub(crate) async fn admit(&self) -> SweepAdmission {
        let now = Instant::now();
        let last_use = self.last_use();
        if now.saturating_duration_since(last_use) < self.cadence {
            return SweepAdmission::Sweep;
        }
        let mut quiet = self.quiet.lock().await;
        match *quiet {
            QuietVerdict::Sweeping { read_at, until } if read_at >= last_use && now < until => return SweepAdmission::Sweep,
            QuietVerdict::Resting { read_at, until } if read_at >= last_use && now < until => return SweepAdmission::Rest,
            QuietVerdict::Unread | QuietVerdict::Sweeping { .. } | QuietVerdict::Resting { .. } => {}
        }
        match self.obligations.earliest_due().await {
            Ok(due) => {
                let (verdict, admission) = self.verdict_for(now, due);
                *quiet = verdict;
                admission
            }
            Err(error) => {
                tracing::warn!(%error, event = "sweep_pacer_obligation_read_failed");
                *quiet = QuietVerdict::Unread;
                SweepAdmission::Sweep
            }
        }
    }

    /// Run `sweep` if this tick is admitted, and do nothing otherwise.
    ///
    /// `sweep` is a future, so a resting tick never starts it and never touches
    /// the database.
    pub(crate) async fn run(&self, sweep: impl Future<Output = Result<(), TaskFailure>>) -> Result<(), TaskFailure> {
        match self.admit().await {
            SweepAdmission::Sweep => sweep.await,
            SweepAdmission::Rest => Ok(()),
        }
    }

    /// The decision a read taken at `now` that found `due` leads to.
    fn verdict_for(&self, now: Instant, due: Option<SystemTime>) -> (QuietVerdict, SweepAdmission) {
        // An instant already past has no duration until it; it is due now.
        match due.map(|due| due.duration_since(SystemTime::now()).unwrap_or(Duration::ZERO)) {
            Some(wait) if wait.is_zero() => (
                QuietVerdict::Sweeping {
                    read_at: now,
                    until: now + self.cadence / 2,
                },
                SweepAdmission::Sweep,
            ),
            Some(wait) => (
                QuietVerdict::Resting {
                    read_at: now,
                    until: now + wait.min(self.ceiling),
                },
                SweepAdmission::Rest,
            ),
            None => (
                QuietVerdict::Resting {
                    read_at: now,
                    until: now + self.ceiling,
                },
                SweepAdmission::Rest,
            ),
        }
    }
}

/// Whole milliseconds from `origin` to `at`, saturating.
fn millis_since(origin: Instant, at: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(origin).as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::sweep_pacer::{ObligationSource, QUIET_CEILING_CADENCES, SweepAdmission, SweepPacer};
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use async_trait::async_trait;
    use std::{
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicUsize, Ordering},
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

    async fn go_quiet() {
        advance(CADENCE).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_tenant_in_use_sweeps_on_every_tick_without_reading_its_obligations() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = SweepPacer::new(CADENCE, Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        advance(CADENCE / 2).await;
        pacer.record_activity();
        advance(CADENCE / 2).await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_quiet_tenant_with_nothing_outstanding_rests_on_one_read_until_the_ceiling() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = SweepPacer::new(CADENCE, Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        go_quiet().await;
        for _ in 0..(QUIET_CEILING_CADENCES - 1) {
            pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Rest);
            advance(CADENCE).await;
        }
        pretty_assertions::assert_eq!(obligations.reads(), 1, "one read serves every quiet tick up to the ceiling");
        advance(CADENCE).await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Rest);
        pretty_assertions::assert_eq!(obligations.reads(), 2, "the ceiling triggers the next read");
    }

    #[tokio::test(start_paused = true)]
    async fn a_quiet_tenant_rests_until_its_earliest_obligation_then_sweeps() {
        let obligations = ScriptedObligations::answering(Some(CADENCE * 3));
        let pacer = SweepPacer::new(CADENCE, Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        go_quiet().await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Rest);
        advance(CADENCE * 2).await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Rest);
        pretty_assertions::assert_eq!(obligations.reads(), 1);
        obligations.answer(Some(Duration::ZERO));
        advance(CADENCE).await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep, "the obligation fell due");
    }

    /// Every sweep of the tenant ticks on its own clock; one read decides for
    /// all of them rather than each reading for itself.
    #[tokio::test(start_paused = true)]
    async fn one_read_admits_every_sweep_while_an_obligation_is_due() {
        let obligations = ScriptedObligations::answering(Some(Duration::ZERO));
        let pacer = SweepPacer::new(CADENCE, Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        go_quiet().await;
        for _ in 0..6 {
            pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        }
        pretty_assertions::assert_eq!(obligations.reads(), 1);
    }

    /// Use can create obligations the last quiet read never saw, so a tenant
    /// going quiet again reads afresh rather than trusting an old rest.
    #[tokio::test(start_paused = true)]
    async fn use_between_quiet_periods_forces_a_fresh_read() {
        let obligations = ScriptedObligations::answering(None);
        let pacer = SweepPacer::new(CADENCE, Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        go_quiet().await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Rest);
        pacer.record_activity();
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        obligations.answer(Some(Duration::ZERO));
        go_quiet().await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unreadable_obligation_falls_back_to_sweeping() {
        let obligations = Arc::new(ScriptedObligations {
            due_in: Mutex::new(None),
            failing: true,
            reads: AtomicUsize::new(0),
        });
        let pacer = SweepPacer::new(CADENCE, Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        go_quiet().await;
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        pretty_assertions::assert_eq!(pacer.admit().await, SweepAdmission::Sweep);
        pretty_assertions::assert_eq!(obligations.reads(), 2, "a failed read is retried on the next tick");
    }
}
