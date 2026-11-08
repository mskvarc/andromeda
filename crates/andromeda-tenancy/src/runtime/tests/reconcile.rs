//! Reconciliation re-probes a dormant tenant and puts its due instant back on
//! the wake schedule without loading it, so the room-bounded wake pass stays
//! the one place a woken tenant is loaded.

use crate::{
    creation_policy::TenantCreationPolicy,
    pending_work::PendingWorkProbe,
    runtime::{
        TenantRuntimeManager,
        tests::{FakeFactory, FakeProbe, FakeRegistry, config, loading_slots, named, wake_timing::TimedSchedule},
    },
    tenant_ref::TenantRef,
    wake_schedule::TenantWakeSchedule,
};
use pretty_assertions::assert_eq;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::time::timeout;

/// The manager the scenarios run.
type Manager = TenantRuntimeManager<FakeRegistry, FakeFactory>;

/// A manager of the tenant `tenant` whose probe answers `probe`, its factory
/// and its schedule.
fn harness(tenant: &str, probe: FakeProbe) -> (Arc<Manager>, Arc<FakeFactory>, Arc<TimedSchedule>) {
    let factory = Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0))));
    let schedule = Arc::new(TimedSchedule::default());
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::new(FakeRegistry::new(&[tenant])),
        Arc::clone(&factory),
        Arc::new(probe) as Arc<dyn PendingWorkProbe>,
        Arc::clone(&schedule) as Arc<dyn TenantWakeSchedule>,
        loading_slots(),
    );
    (manager, factory, schedule)
}

#[tokio::test]
async fn reconciling_a_tenant_with_work_due_records_its_due_instant_without_loading_it() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let (manager, factory, schedule) = harness("overdue", FakeProbe::due_at(due));

    manager.reconcile(&named("overdue")).await;

    assert_eq!(schedule.wake_of("overdue"), Some(due));
    assert_eq!(factory.built.load(Ordering::Relaxed), 0, "the wake pass loads it, not the reconciliation");
}

#[tokio::test]
async fn reconciling_a_tenant_whose_work_is_ahead_records_that_instant() {
    let due = SystemTime::now() + Duration::from_mins(5);
    let (manager, _, schedule) = harness("ahead", FakeProbe::due_at(due));

    manager.reconcile(&named("ahead")).await;

    assert_eq!(schedule.wake_of("ahead"), Some(due));
}

#[tokio::test]
async fn reconciling_a_tenant_with_nothing_due_clears_its_schedule() {
    let (manager, _, schedule) = harness("idle", FakeProbe::none());
    schedule.write(&named("idle"), Some(SystemTime::now()));

    manager.reconcile(&named("idle")).await;

    assert_eq!(schedule.wake_of("idle"), None);
}

#[tokio::test]
async fn reconciling_a_resident_tenant_clears_its_schedule() {
    let (manager, factory, schedule) = harness("resident", FakeProbe::due_at(SystemTime::now()));
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("resident")), TenantCreationPolicy::MustExist)
            .await
            .expect("the tenant resolves"),
    );
    schedule.write(&named("resident"), Some(SystemTime::now()));

    manager.reconcile(&named("resident")).await;

    assert_eq!(schedule.wake_of("resident"), None, "a resident tenant runs its own drains");
    assert_eq!(factory.built.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn an_established_tenant_with_an_unmigrated_schema_is_recorded_due_at_once() {
    let (manager, factory, schedule) = harness("behind", FakeProbe::unmigrated());
    let before = SystemTime::now();

    manager.reconcile(&named("behind")).await;

    assert!(
        schedule.wake_of("behind").is_some_and(|at| at >= before && at <= SystemTime::now()),
        "building its runtime migrates it, so it is due now"
    );
    assert_eq!(factory.built.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn reconciling_a_due_tenant_rings_a_waker_sleeping_past_its_instant() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let (manager, _, _) = harness("overdue", FakeProbe::due_at(due));
    let alarm = manager.wake_alarm();
    alarm.begin_pass();
    alarm.sleep_until(Some(SystemTime::now() + Duration::from_mins(10)));

    manager.reconcile(&named("overdue")).await;

    assert!(timeout(Duration::from_millis(100), alarm.rung()).await.is_ok());
}
