//! A wake pass that finds every place taken asks the woken runtimes whether
//! their work has run, and each that has gives up its place before its woken
//! lifetime ends, so a full cache loads due tenants as fast as their work
//! drains rather than one per woken lifetime.

use crate::{
    creation_policy::TenantCreationPolicy,
    due_work::DueWork,
    pending_work::PendingWorkProbe,
    runtime::{
        TenantRuntimeManager,
        tests::{
            FakeFactory,
            FakeProbe,
            FakeRegistry,
            config,
            named,
            wake_pass::FakeCatchUp,
            wake_timing::{TimedSchedule, resident, run_waker},
        },
        wake_pass::{NextWakePass, WakePassReport},
    },
    tenant_ref::TenantRef,
    wake_schedule::TenantWakeSchedule,
};
use andromeda_configuration::human_duration::HumanDuration;
use andromeda_telemetry::tenancy::WakeOutcome;
use pretty_assertions::assert_eq;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::time::{sleep, timeout};

/// The manager the scenarios run.
type Manager = TenantRuntimeManager<FakeRegistry, FakeFactory>;

/// How long a claim is leased in these scenarios.
const LEASE: Duration = Duration::from_mins(1);

/// A manager, its factory and its schedule.
struct ReleaseHarness {
    /// The manager under test.
    manager: Arc<Manager>,
    /// The factory its runtimes come from, holding each tenant's due-work
    /// answer.
    factory: Arc<FakeFactory>,
    /// The wake schedule the passes claim from.
    schedule: Arc<TimedSchedule>,
}

/// A manager of `registered` tenants keeping at most `max_resident` loaded, a
/// woken tenant loaded for a minute and loading at most `per_pass` tenants a
/// pass, whose tenants all have work due now.
fn harness(registered: &[&str], max_resident: usize, per_pass: usize) -> ReleaseHarness {
    let mut tenancy = config(true, max_resident);
    tenancy.residency.woken_lifetime = HumanDuration::from_secs(60);
    tenancy.residency.max_wakes_per_pass = NonZeroUsize::new(per_pass).expect("a positive budget");
    let factory = Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0))));
    let schedule = Arc::new(TimedSchedule::default());
    let manager = TenantRuntimeManager::new(
        tenancy,
        Arc::new(FakeRegistry::new(registered)),
        Arc::clone(&factory),
        Arc::new(FakeProbe::due_as_answered()) as Arc<dyn PendingWorkProbe>,
        Arc::clone(&schedule) as Arc<dyn TenantWakeSchedule>,
    );
    ReleaseHarness { manager, factory, schedule }
}

/// One pass at the current instant.
async fn pass(manager: &Manager) -> WakePassReport {
    let now = SystemTime::now();
    manager
        .run_wake_pass(&FakeCatchUp::listing(&[]), now, now + LEASE)
        .await
        .expect("the pass completes")
}

/// `name`, as the cache keys it.
fn tenant(name: &str) -> TenantRef {
    TenantRef::Named(named(name))
}

/// Make every tenant of `names` due now.
fn due_now(schedule: &TimedSchedule, names: &[&str]) {
    let now = SystemTime::now();
    for name in names {
        schedule.write(&named(name), Some(now));
    }
}

#[tokio::test]
async fn a_woken_runtime_whose_work_has_run_gives_up_its_place_to_a_due_wake() {
    let ReleaseHarness { manager, factory, schedule } = harness(&["woken", "waiting"], 1, 4);
    due_now(&schedule, &["woken"]);
    pass(&manager).await;
    factory.due_work(&tenant("woken")).answer(DueWork::Drained);
    due_now(&schedule, &["waiting"]);

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 1);
    assert!(resident(&manager, "waiting"));
    assert!(!resident(&manager, "woken"), "the drained runtime gave up its place");
    assert_eq!(factory.due_work(&tenant("woken")).asked(), 1);
}

#[tokio::test]
async fn a_woken_runtime_still_working_keeps_its_place_and_is_asked_again_only_after_its_wait() {
    let ReleaseHarness { manager, factory, schedule } = harness(&["woken", "waiting"], 1, 4);
    due_now(&schedule, &["woken"]);
    pass(&manager).await;
    due_now(&schedule, &["waiting"]);
    let now = SystemTime::now();

    let first = pass(&manager).await;
    let second = pass(&manager).await;

    assert_eq!((first.scheduled, second.scheduled), (0, 0));
    assert!(resident(&manager, "woken"));
    assert_eq!(factory.due_work(&tenant("woken")).asked(), 1, "the second pass falls inside the wait");
    assert!(
        matches!(first.next, NextWakePass::At(at) if at <= now + Duration::from_millis(150)),
        "the waker comes back as the wait ends, not as the woken lifetime does: {:?}",
        first.next
    );
}

#[tokio::test]
async fn a_cache_with_room_asks_no_woken_runtime() {
    let ReleaseHarness { manager, factory, schedule } = harness(&["woken", "waiting"], 3, 4);
    due_now(&schedule, &["woken"]);
    pass(&manager).await;
    factory.due_work(&tenant("woken")).answer(DueWork::Drained);
    due_now(&schedule, &["waiting"]);

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 1);
    assert!(resident(&manager, "woken") && resident(&manager, "waiting"));
    assert_eq!(
        factory.due_work(&tenant("woken")).asked(),
        0,
        "with room, a woken runtime keeps its whole lifetime"
    );
}

#[tokio::test]
async fn a_full_cache_with_no_wake_due_asks_no_woken_runtime() {
    let ReleaseHarness { manager, factory, schedule } = harness(&["woken"], 1, 4);
    due_now(&schedule, &["woken"]);
    pass(&manager).await;
    factory.due_work(&tenant("woken")).answer(DueWork::Drained);

    let report = pass(&manager).await;

    assert_eq!((report.scheduled, report.overdue, report.next), (0, None, NextWakePass::WhenRecorded));
    assert_eq!(factory.due_work(&tenant("woken")).asked(), 0, "no due wake would take the place");
    assert!(resident(&manager, "woken"));
}

#[tokio::test]
async fn a_wake_with_no_place_left_asks_a_woken_runtime_before_it_defers() {
    let ReleaseHarness { manager, factory, .. } = harness(&["woken", "next"], 1, 4);
    assert_eq!(manager.wake(&named("woken")).await, WakeOutcome::Loaded);
    factory.due_work(&tenant("woken")).answer(DueWork::Drained);

    let outcome = manager.wake(&named("next")).await;

    assert_eq!(outcome, WakeOutcome::Loaded, "the drained woken runtime gave up its place");
    assert!(resident(&manager, "next") && !resident(&manager, "woken"));
    assert_eq!(factory.due_work(&tenant("woken")).asked(), 1);
}

#[tokio::test]
async fn a_wake_with_no_place_left_defers_when_the_woken_runtime_is_still_working() {
    let ReleaseHarness { manager, factory, .. } = harness(&["woken", "next"], 1, 4);
    assert_eq!(manager.wake(&named("woken")).await, WakeOutcome::Loaded);

    let outcome = manager.wake(&named("next")).await;

    assert_eq!(outcome, WakeOutcome::Deferred);
    assert!(resident(&manager, "woken") && !resident(&manager, "next"));
    assert_eq!(factory.due_work(&tenant("woken")).asked(), 1);
}

#[tokio::test]
async fn a_woken_runtime_a_request_holds_is_not_asked() {
    let ReleaseHarness { manager, factory, schedule } = harness(&["woken", "waiting"], 1, 4);
    due_now(&schedule, &["woken"]);
    pass(&manager).await;
    factory.due_work(&tenant("woken")).answer(DueWork::Drained);
    let held = manager
        .resolve_state(&tenant("woken"), TenantCreationPolicy::MustExist)
        .await
        .expect("the tenant resolves");
    due_now(&schedule, &["waiting"]);

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 0);
    assert_eq!(factory.due_work(&tenant("woken")).asked(), 0);
    drop(held);
}

#[tokio::test]
async fn a_pass_frees_no_more_places_than_it_may_fill() {
    let woken = ["w1", "w2", "w3", "w4", "w5", "w6"];
    let waiting = ["d1", "d2", "d3", "d4", "d5", "d6"];
    let registered: Vec<&str> = woken.iter().chain(&waiting).copied().collect();
    let ReleaseHarness { manager, factory, schedule } = harness(&registered, 6, 4);
    due_now(&schedule, &woken);
    assert_eq!(
        pass(&manager).await.scheduled + pass(&manager).await.scheduled,
        6,
        "two passes fill the six places"
    );
    for name in woken {
        factory.due_work(&tenant(name)).answer(DueWork::Drained);
    }
    due_now(&schedule, &waiting);

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 4);
    let asked: usize = woken.iter().map(|name| factory.due_work(&tenant(name)).asked()).sum();
    assert_eq!(asked, 4, "asking stops once as many places are free as one pass may fill");
}

#[tokio::test]
async fn the_woken_runtime_woken_longest_ago_is_asked_first() {
    let ReleaseHarness { manager, factory, schedule } = harness(&["older", "newer", "waiting"], 2, 1);
    due_now(&schedule, &["older"]);
    pass(&manager).await;
    sleep(Duration::from_millis(20)).await;
    due_now(&schedule, &["newer"]);
    pass(&manager).await;
    factory.due_work(&tenant("older")).answer(DueWork::Drained);
    factory.due_work(&tenant("newer")).answer(DueWork::Drained);
    due_now(&schedule, &["waiting"]);

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 1);
    assert_eq!((factory.due_work(&tenant("older")).asked(), factory.due_work(&tenant("newer")).asked()), (1, 0));
    assert!(!resident(&manager, "older") && resident(&manager, "newer"));
}

#[tokio::test]
async fn a_full_cache_loads_due_tenants_as_fast_as_their_work_drains() {
    let names = ["first", "second", "third", "fourth", "fifth", "sixth"];
    let ReleaseHarness { manager, factory, schedule } = harness(&names, 2, 4);
    for name in names {
        factory.due_work(&tenant(name)).answer(DueWork::Drained);
    }
    due_now(&schedule, &names);
    let waker = tokio::spawn(run_waker(Arc::clone(&manager)));

    let all_loaded = timeout(Duration::from_secs(3), async {
        while factory.built.load(Ordering::Relaxed) < names.len() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    waker.abort();

    assert!(
        all_loaded.is_ok(),
        "six tenants through two places in a few seconds, not three one-minute lifetimes; {} built",
        factory.built.load(Ordering::Relaxed)
    );
}
