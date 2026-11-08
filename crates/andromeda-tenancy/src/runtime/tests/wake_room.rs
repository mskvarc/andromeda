//! A wake pass claims only as many due tenants as the cache has room for, and
//! with no room claims and probes none and sleeps until a place frees, so a
//! full cache costs no probe of a tenant that could not be loaded and delays
//! no wake past the instant room opens.

use crate::{
    creation_policy::TenantCreationPolicy,
    pending_work::PendingWorkProbe,
    runtime::{
        TenantRuntimeManager,
        tests::{
            FakeFactory,
            FakeProbe,
            FakeRegistry,
            config,
            named,
            provisioning_slots,
            wake_pass::FakeCatchUp,
            wake_timing::{TimedSchedule, resident, run_waker},
        },
        wake_pass::{NextWakePass, WakePassReport},
    },
    tenant_ref::TenantRef,
    wake_backlog::OverdueWakes,
    wake_schedule::TenantWakeSchedule,
};
use andromeda_configuration::human_duration::HumanDuration;
use pretty_assertions::assert_eq;
use std::{
    num::NonZeroUsize,
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, SystemTime},
};
use tokio::time::{sleep, timeout};

/// The manager the scenarios run.
type Manager = TenantRuntimeManager<FakeRegistry, FakeFactory>;

/// How long a claim is leased in these scenarios.
const LEASE: Duration = Duration::from_mins(1);

/// A manager, its probe and its schedule.
struct RoomHarness {
    /// The manager under test.
    manager: Arc<Manager>,
    /// The probe every wake reads, counting its reads.
    probe: Arc<FakeProbe>,
    /// The wake schedule the passes claim from.
    schedule: Arc<TimedSchedule>,
}

/// A manager of `registered` tenants keeping at most `max_resident` loaded and
/// a woken tenant loaded for `woken_lifetime`, whose tenants all have work due
/// since `due`.
fn harness(registered: &[&str], max_resident: usize, woken_lifetime: HumanDuration, due: SystemTime) -> RoomHarness {
    let mut tenancy = config(true, max_resident);
    tenancy.residency.woken_lifetime = woken_lifetime;
    let probe = Arc::new(FakeProbe::due_at(due));
    let schedule = Arc::new(TimedSchedule::default());
    let manager = TenantRuntimeManager::new(
        tenancy,
        Arc::new(FakeRegistry::new(registered)),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::clone(&probe) as Arc<dyn PendingWorkProbe>,
        Arc::clone(&schedule) as Arc<dyn TenantWakeSchedule>,
        provisioning_slots(),
    );
    RoomHarness { manager, probe, schedule }
}

/// One pass at the current instant.
async fn pass(manager: &Manager) -> WakePassReport {
    let now = SystemTime::now();
    manager
        .run_wake_pass(&FakeCatchUp::listing(&[]), now, now + LEASE)
        .await
        .expect("the pass completes")
}

/// Hold every tenant of `tenants` due at `due`.
fn hold_due(schedule: &TimedSchedule, tenants: &[String], due: SystemTime) {
    for tenant in tenants {
        schedule.write(&named(tenant), Some(due));
    }
}

/// The tenant names `<prefix>1` to `<prefix><count>`.
fn names(prefix: &str, count: usize) -> Vec<String> {
    (1..=count).map(|index| format!("{prefix}{index}")).collect()
}

/// `woken` and `waiting` together, as the registry takes them.
fn registered<'a>(woken: &'a [String], waiting: &'a [String]) -> Vec<&'a str> {
    woken.iter().chain(waiting).map(String::as_str).collect()
}

#[tokio::test]
async fn a_pass_with_no_room_claims_no_wake_and_probes_no_tenant() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let (woken, waiting) = (names("woken", 3), names("waiting", 10));
    let RoomHarness { manager, probe, schedule } = harness(&registered(&woken, &waiting), 3, HumanDuration::from_secs(60), due);
    hold_due(&schedule, &woken, due);
    assert_eq!(pass(&manager).await.scheduled, 3, "the three places fill with woken tenants");
    hold_due(&schedule, &waiting, due);
    let probes = probe.asked().len();
    let now = SystemTime::now();

    let report = pass(&manager).await;

    assert_eq!((report.scheduled, report.caught_up), (0, 0));
    assert_eq!(probe.asked().len(), probes, "no tenant is probed only to be deferred");
    assert!(
        matches!(report.next, NextWakePass::At(at) if at <= now + Duration::from_millis(150)),
        "the woken runtimes answered their work is outstanding, so the waker sleeps until the first may be asked again, \
         got {:?}",
        report.next
    );
    assert_eq!(report.overdue, NonZeroUsize::new(10).map(|count| OverdueWakes::new(count, due)));
}

#[tokio::test]
async fn an_unclaimed_wake_keeps_its_due_instant() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let (woken, waiting) = (names("woken", 1), names("waiting", 2));
    let RoomHarness { manager, schedule, .. } = harness(&registered(&woken, &waiting), 1, HumanDuration::from_secs(60), due);
    hold_due(&schedule, &woken, due);
    pass(&manager).await;
    hold_due(&schedule, &waiting, due);

    pass(&manager).await;

    for tenant in &waiting {
        assert_eq!(schedule.wake_of(tenant), Some(due), "{tenant} is neither leased nor moved to a retry");
    }
}

#[tokio::test]
async fn a_pass_claims_only_as_many_wakes_as_it_has_room_for() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let (woken, waiting) = (names("woken", 1), names("waiting", 10));
    let RoomHarness { manager, probe, schedule } = harness(&registered(&woken, &waiting), 3, HumanDuration::from_secs(60), due);
    hold_due(&schedule, &woken, due);
    pass(&manager).await;
    hold_due(&schedule, &waiting, due);
    let probes = probe.asked().len();

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 2, "two places are free beside the woken tenant");
    assert_eq!(probe.asked().len(), probes + 2, "only the claimed tenants are probed");
    assert_eq!(report.next, NextWakePass::Immediately, "the room was spent, so more wakes may be due");
    assert_eq!(
        waiting.iter().filter(|tenant| schedule.wake_of(tenant) == Some(due)).count(),
        8,
        "the eight left keep their due instant"
    );
}

#[tokio::test]
async fn a_runtime_past_its_woken_lifetime_counts_as_room() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let RoomHarness { manager, schedule, .. } = harness(&["first", "second"], 1, HumanDuration::from_millis(200), due);
    schedule.write(&named("first"), Some(due));
    pass(&manager).await;
    sleep(Duration::from_millis(300)).await;
    schedule.write(&named("second"), Some(due));

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 1);
    assert!(resident(&manager, "second"));
    assert!(!resident(&manager, "first"), "the runtime past its woken lifetime gave up its place");
}

#[tokio::test]
async fn a_leased_runtime_is_not_room_and_letting_it_go_rings_the_waker() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let RoomHarness { manager, probe, schedule } = harness(&["busy", "waiting"], 1, HumanDuration::from_secs(60), due);
    let held = manager
        .resolve_state(&TenantRef::Named(named("busy")), TenantCreationPolicy::MustExist)
        .await
        .expect("the tenant resolves");
    schedule.write(&named("waiting"), Some(due));

    let report = pass(&manager).await;

    assert_eq!((report.scheduled, report.next), (0, NextWakePass::WhenRoomOpens));
    assert!(probe.asked().is_empty(), "no tenant is probed while a request holds the only place");
    drop(held);
    assert!(
        timeout(Duration::from_millis(100), manager.wake_alarm().rung()).await.is_ok(),
        "the request letting go of its tenant wakes the waker"
    );
}

#[tokio::test]
async fn an_eviction_that_frees_a_place_rings_a_waker_sleeping_for_room() {
    let due = SystemTime::now() - Duration::from_secs(30);
    let RoomHarness { manager, schedule, .. } = harness(&["woken", "waiting"], 1, HumanDuration::from_secs(60), due);
    schedule.write(&named("woken"), Some(due));
    pass(&manager).await;
    schedule.write(&named("waiting"), Some(due));
    assert!(matches!(pass(&manager).await.next, NextWakePass::At(_)));

    assert!(manager.evict_named(&TenantRef::Named(named("woken"))).await);

    assert!(timeout(Duration::from_millis(100), manager.wake_alarm().rung()).await.is_ok());
}

#[tokio::test]
async fn the_wake_due_longest_is_loaded_first_when_room_opens() {
    let now = SystemTime::now();
    let RoomHarness { manager, schedule, .. } = harness(&["woken", "oldest", "older", "newest"], 1, HumanDuration::from_millis(200), now);
    schedule.write(&named("woken"), Some(now));
    pass(&manager).await;
    schedule.write(&named("newest"), Some(now - Duration::from_secs(10)));
    schedule.write(&named("older"), Some(now - Duration::from_secs(20)));
    schedule.write(&named("oldest"), Some(now - Duration::from_secs(30)));
    pass(&manager).await;
    sleep(Duration::from_millis(300)).await;

    let report = pass(&manager).await;

    assert_eq!(report.scheduled, 1);
    assert!(resident(&manager, "oldest"));
    assert!(!resident(&manager, "older") && !resident(&manager, "newest"));
}

#[tokio::test]
async fn a_wake_is_loaded_the_moment_a_woken_lifetime_ends() {
    let lifetime = Duration::from_secs(1);
    let RoomHarness { manager, schedule, .. } = harness(&["first", "second", "late"], 2, HumanDuration::from_secs(1), SystemTime::now());
    let loaded_at = SystemTime::now();
    schedule.write(&named("first"), Some(loaded_at));
    schedule.write(&named("second"), Some(loaded_at));
    let waker = tokio::spawn(run_waker(Arc::clone(&manager)));
    timeout(Duration::from_secs(5), async {
        while !(resident(&manager, "first") && resident(&manager, "second")) {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the two due tenants are woken");
    manager.reconcile(&named("late")).await;

    let served = timeout(Duration::from_secs(5), async {
        while !resident(&manager, "late") {
            sleep(Duration::from_millis(5)).await;
        }
        SystemTime::now()
    })
    .await
    .expect("the late tenant is woken once room opens");
    waker.abort();

    let lifetime_end = loaded_at + lifetime;
    let lateness = served.duration_since(lifetime_end).unwrap_or_default();
    assert!(
        lateness <= Duration::from_millis(250),
        "loaded {lateness:?} after the first woken lifetime ended"
    );
}
