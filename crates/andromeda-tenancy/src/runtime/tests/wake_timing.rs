//! When the waker runs its passes: at once after a pass that claimed its whole
//! budget, and otherwise at the earliest wake the schedule holds or when this
//! process records an earlier one.
//!
//! Every scenario runs at the default `max_wakes_per_pass` of 4.

use crate::{
    creation_policy::TenantCreationPolicy,
    runtime::{
        TenantRuntimeManager,
        tests::{FakeFactory, FakeProbe, FakeRegistry, FakeSchedule, config, named, wake_pass::FakeCatchUp},
        wake_pass::NextWakePass,
    },
    tenant_ref::TenantRef,
    wake_backlog::OverdueWakes,
    wake_schedule::{TenantWakeSchedule, WakeScheduleError},
};
use andromeda_configuration::human_duration::HumanDuration;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex as StdMutex, atomic::AtomicUsize},
    time::{Duration, SystemTime},
};
use tokio::time::{sleep, timeout};

/// The manager the scenarios run.
type Manager = TenantRuntimeManager<FakeRegistry, FakeFactory>;

/// How long a claim is leased in these scenarios.
const LEASE: Duration = Duration::from_mins(1);

/// A wake schedule holding each tenant's wake instant, claimed by time as the
/// platform table is.
#[derive(Default)]
pub(super) struct TimedSchedule {
    /// Each tenant's next wake, `None` when it has none.
    wakes: StdMutex<HashMap<String, Option<SystemTime>>>,
}

impl TimedSchedule {
    /// Write `tenant`'s next wake.
    pub(super) fn write(&self, tenant: &TenantName, wake_at: Option<SystemTime>) {
        if let Ok(mut wakes) = self.wakes.lock() {
            wakes.insert(tenant.as_str().to_string(), wake_at);
        }
    }
}

#[async_trait]
impl TenantWakeSchedule for TimedSchedule {
    async fn record(&self, tenant: &TenantName, _probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        self.write(tenant, wake_at);
        Ok(())
    }

    async fn claim_due(&self, now: SystemTime, lease_until: SystemTime, limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        let Ok(mut wakes) = self.wakes.lock() else {
            return Ok(Vec::new());
        };
        let mut due: Vec<(SystemTime, String)> = wakes
            .iter()
            .filter_map(|(tenant, wake_at)| wake_at.filter(|at| *at <= now).map(|at| (at, tenant.clone())))
            .collect();
        due.sort();
        due.truncate(limit);
        for (_, tenant) in &due {
            wakes.insert(tenant.clone(), Some(lease_until));
        }
        Ok(due.into_iter().map(|(_, tenant)| named(&tenant)).collect())
    }

    async fn earliest_wake(&self) -> Result<Option<SystemTime>, WakeScheduleError> {
        Ok(self.wakes.lock().ok().and_then(|wakes| wakes.values().flatten().min().copied()))
    }

    async fn overdue(&self, now: SystemTime) -> Result<Option<OverdueWakes>, WakeScheduleError> {
        let Ok(wakes) = self.wakes.lock() else {
            return Ok(None);
        };
        let due: Vec<SystemTime> = wakes.values().flatten().copied().filter(|at| *at <= now).collect();
        Ok(NonZeroUsize::new(due.len())
            .zip(due.iter().min().copied())
            .map(|(count, oldest)| OverdueWakes::new(count, oldest)))
    }

    async fn settle(&self, tenant: &TenantName, _probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        self.write(tenant, wake_at);
        Ok(())
    }

    async fn next_unprobed(&self, _limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        Ok(Vec::new())
    }
}

/// Run wake passes the way the broker's waker does, until dropped: at once
/// after a full pass, otherwise asleep until the reported instant or a ring.
async fn run_waker(manager: Arc<Manager>) {
    let catch_up = FakeCatchUp::listing(&[]);
    loop {
        let now = SystemTime::now();
        let report = manager.run_wake_pass(&catch_up, now, now + LEASE).await.expect("the pass completes");
        match report.next {
            NextWakePass::Immediately => {}
            NextWakePass::At(at) => {
                let wait = at.duration_since(SystemTime::now()).unwrap_or_default();
                tokio::select! {
                    () = sleep(wait) => {}
                    () = manager.wake_alarm().rung() => {}
                }
            }
            NextWakePass::WhenRecorded => manager.wake_alarm().rung().await,
        }
    }
}

/// Whether `name` has a runtime in the cache.
fn resident(manager: &Manager, name: &str) -> bool {
    manager.runtimes.contains_key(&TenantRef::Named(named(name)))
}

#[tokio::test]
async fn a_pass_that_claims_its_whole_budget_is_followed_at_once_until_every_due_tenant_is_loaded() {
    let names: Vec<String> = (1..=12).map(|index| format!("due{index}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::new(FakeRegistry::new(&names)),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::new(FakeProbe::due_as_answered()),
        Arc::new(FakeSchedule::claiming(&names)),
    );
    let catch_up = FakeCatchUp::listing(&[]);

    let mut nexts = Vec::new();
    for _ in 0..3 {
        let now = SystemTime::now();
        nexts.push(manager.run_wake_pass(&catch_up, now, now + LEASE).await.expect("the pass completes").next);
    }

    assert_eq!(nexts[..2], [NextWakePass::Immediately, NextWakePass::Immediately]);
    assert!(names.iter().all(|name| resident(&manager, name)), "three passes load all 12 due tenants");
}

#[tokio::test]
async fn a_pass_that_leaves_budget_over_reports_the_earliest_wake_the_schedule_holds() {
    let schedule = Arc::new(FakeSchedule::claiming(&["due1"]));
    let earliest = SystemTime::now() + Duration::from_mins(5);
    schedule.hold_earliest(Some(earliest));
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::new(FakeRegistry::new(&["due1"])),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::new(FakeProbe::due_as_answered()),
        Arc::clone(&schedule) as Arc<dyn TenantWakeSchedule>,
    );
    let catch_up = FakeCatchUp::listing(&[]);
    let now = SystemTime::now();

    let first = manager.run_wake_pass(&catch_up, now, now + LEASE).await.expect("the pass completes");
    schedule.hold_earliest(None);
    let second = manager.run_wake_pass(&catch_up, now, now + LEASE).await.expect("the pass completes");

    assert_eq!((first.scheduled, first.next), (1, NextWakePass::At(earliest)));
    assert_eq!((second.scheduled, second.next), (0, NextWakePass::WhenRecorded));
}

#[tokio::test]
async fn a_tenant_unloaded_with_a_wake_recorded_for_t_and_no_other_wake_pending_is_loaded_within_a_second_of_t() {
    let mut tenancy = config(true, 1);
    tenancy.residency.min_wake_interval = HumanDuration::from_secs(1);
    let due = SystemTime::now() + Duration::from_secs(3);
    let manager = TenantRuntimeManager::new(
        tenancy,
        Arc::new(FakeRegistry::new(&["periodic", "other"])),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::new(FakeProbe::due_at(due)),
        Arc::new(TimedSchedule::default()),
    );
    let waker = tokio::spawn(run_waker(Arc::clone(&manager)));
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("periodic")), TenantCreationPolicy::MustExist)
            .await
            .expect("the tenant resolves"),
    );
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("other")), TenantCreationPolicy::MustExist)
            .await
            .expect("the tenant resolves"),
    );

    let loaded = timeout(Duration::from_secs(10), async {
        while !resident(&manager, "periodic") {
            sleep(Duration::from_millis(10)).await;
        }
        SystemTime::now()
    })
    .await
    .expect("the unloaded tenant is woken");
    waker.abort();

    let lateness = loaded.duration_since(due).expect("the tenant is not loaded before its wake");
    assert!(lateness <= Duration::from_secs(1), "loaded {lateness:?} after its wake");
}
