//! A stopping broker tears every loaded runtime down at once, and none of
//! them past its stop deadline.

use crate::{
    creation_policy::TenantCreationPolicy,
    due_work::{DueWork, DueWorkSource},
    error::TenancyError,
    runtime::{
        TenantRuntimeManager,
        tests::{FakeDueWork, FakeProbe, FakeRegistry, FakeSchedule, Unobserved, bounded_config, loading_slots, named},
    },
    state_factory::{RuntimeActivity, TenantRuntime, TenantStateFactory},
    teardown_bound::TeardownBound,
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use async_trait::async_trait;
use pretty_assertions::assert_eq;
use std::{
    sync::{
        Arc,
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::time::{Instant, sleep, sleep_until};

/// The loaded tenants of a stopping broker, one more than two rounds of the
/// default eviction bound.
const TENANTS: usize = 17;

/// What every teardown of one test observed.
#[derive(Default)]
struct Observed {
    /// Teardowns running now.
    active: AtomicUsize,
    /// The most teardowns that ran at once.
    peak: AtomicUsize,
    /// The bound each teardown was given.
    bounds: StdMutex<Vec<TeardownBound>>,
}

/// Whether a teardown stops at the deadline it is given.
#[derive(Clone, Copy)]
enum DeadlineHandling {
    /// It stops by the deadline, as a runtime whose tasks are cut off does.
    Honoured,
    /// It takes its whole time whatever the deadline.
    Ignored,
}

/// A runtime whose teardown takes `hold`.
struct TimedTeardown {
    /// How long the teardown takes.
    hold: Duration,
    /// Whether it stops at its deadline.
    handling: DeadlineHandling,
    /// Shared with every runtime of the test.
    observed: Arc<Observed>,
}

#[async_trait]
impl TenantRuntime for TimedTeardown {
    async fn shutdown(self, bound: TeardownBound) {
        self.observed.bounds.lock().expect("bounds lock").push(bound);
        let running = self.observed.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.observed.peak.fetch_max(running, Ordering::SeqCst);
        match (bound, self.handling) {
            (TeardownBound::Deadline(deadline), DeadlineHandling::Honoured) => sleep_until(deadline.min(Instant::now() + self.hold)).await,
            (TeardownBound::Deadline(_), DeadlineHandling::Ignored) | (TeardownBound::GracePeriods, _) => sleep(self.hold).await,
        }
        self.observed.active.fetch_sub(1, Ordering::SeqCst);
    }

    fn activity(&self) -> Arc<dyn RuntimeActivity> {
        Arc::new(Unobserved)
    }

    fn next_periodic_due(&self) -> Option<SystemTime> {
        None
    }

    fn undelivered_changes(&self) -> usize {
        0
    }

    fn due_work(&self) -> Arc<dyn DueWorkSource> {
        Arc::new(FakeDueWork::answering(DueWork::Outstanding))
    }
}

/// Builds [`TimedTeardown`] runtimes.
struct TimedFactory {
    /// How long each teardown takes.
    hold: Duration,
    /// Whether each stops at its deadline.
    handling: DeadlineHandling,
    /// Shared with every runtime it builds.
    observed: Arc<Observed>,
}

#[async_trait]
impl TenantStateFactory for TimedFactory {
    type State = String;
    type Runtime = TimedTeardown;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, TimedTeardown), TenancyError> {
        Ok((
            Arc::new(database.as_str().to_owned()),
            TimedTeardown {
                hold: self.hold,
                handling: self.handling,
                observed: Arc::clone(&self.observed),
            },
        ))
    }
}

/// A manager holding `TENANTS` loaded runtimes under an eviction bound of
/// two, each tearing down as given.
async fn loaded(hold: Duration, handling: DeadlineHandling) -> (Arc<TenantRuntimeManager<FakeRegistry, TimedFactory>>, Arc<Observed>) {
    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let observed = Arc::new(Observed::default());
    let manager = TenantRuntimeManager::new(
        bounded_config(TENANTS, 8, 2),
        Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>())),
        Arc::new(TimedFactory {
            hold,
            handling,
            observed: Arc::clone(&observed),
        }),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
        loading_slots(),
    );
    for name in &names {
        drop(
            manager
                .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
                .await
                .expect("resolves"),
        );
    }
    (manager, observed)
}

/// Torn down a slot at a time, 17 one-second teardowns under a bound of two
/// would take nine seconds.
#[tokio::test(start_paused = true)]
async fn a_stopping_broker_tears_every_loaded_runtime_down_at_once_by_its_deadline() {
    let (manager, observed) = loaded(Duration::from_secs(1), DeadlineHandling::Honoured).await;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(10);
    manager.shutdown_all(deadline).await;
    assert_eq!(started.elapsed(), Duration::from_secs(1));
    assert_eq!(observed.peak.load(Ordering::SeqCst), TENANTS);
    assert_eq!(*observed.bounds.lock().expect("bounds lock"), vec![TeardownBound::Deadline(deadline); TENANTS]);
    assert_eq!(manager.runtimes.len(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_runtime_still_tearing_down_at_the_deadline_is_abandoned_then() {
    let (manager, _observed) = loaded(Duration::from_secs(60), DeadlineHandling::Ignored).await;
    let started = Instant::now();
    manager.shutdown_all(started + Duration::from_secs(3)).await;
    assert_eq!(started.elapsed(), Duration::from_secs(3));
    assert_eq!(manager.runtimes.len(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_stopping_broker_with_nothing_loaded_stops_at_once() {
    let (manager, _observed) = loaded(Duration::from_secs(1), DeadlineHandling::Honoured).await;
    manager.shutdown_all(Instant::now() + Duration::from_secs(10)).await;
    let started = Instant::now();
    manager.shutdown_all(started + Duration::from_secs(10)).await;
    assert_eq!(started.elapsed(), Duration::ZERO);
}
