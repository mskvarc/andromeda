//! The wakes a pass reports overdue: left unclaimed by a pass that spent its
//! whole budget or had no room for them, or deferred because a request took
//! the room, each overdue since its work fell due.

use crate::{
    runtime::{
        TenantRuntimeManager,
        tests::{FakeFactory, FakeProbe, FakeRegistry, config, named, wake_pass::FakeCatchUp, wake_timing::TimedSchedule},
        wake_pass::WakePassReport,
    },
    wake_backlog::OverdueWakes,
};
use andromeda_configuration::human_duration::HumanDuration;
use pretty_assertions::assert_eq;
use std::{
    num::NonZeroUsize,
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, SystemTime},
};
use tokio::time::sleep;

/// The manager the scenarios run.
type Manager = TenantRuntimeManager<FakeRegistry, FakeFactory>;

/// How long a claim is leased in these scenarios.
const LEASE: Duration = Duration::from_mins(1);

/// A manager of `registered` tenants keeping at most `max_resident` loaded and
/// a woken tenant loaded for `woken_lifetime`, whose tenants all have work due
/// since `due`, over `schedule`.
fn manager(registered: &[&str], max_resident: usize, woken_lifetime: HumanDuration, due: SystemTime, schedule: Arc<TimedSchedule>) -> Arc<Manager> {
    let mut tenancy = config(true, max_resident);
    tenancy.residency.woken_lifetime = woken_lifetime;
    TenantRuntimeManager::new(
        tenancy,
        Arc::new(FakeRegistry::new(registered)),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::new(FakeProbe::due_at(due)),
        schedule,
    )
}

/// One pass at `now`.
async fn pass(manager: &Manager, now: SystemTime) -> WakePassReport {
    manager
        .run_wake_pass(&FakeCatchUp::listing(&[]), now, now + LEASE)
        .await
        .expect("the pass completes")
}

fn overdue(count: usize, oldest_due: SystemTime) -> Option<OverdueWakes> {
    NonZeroUsize::new(count).map(|count| OverdueWakes::new(count, oldest_due))
}

#[tokio::test]
async fn a_pass_capped_by_room_reports_the_due_wakes_it_left_unclaimed_overdue_since_their_work_fell_due() {
    let now = SystemTime::now();
    let due = now - Duration::from_secs(30);
    let schedule = Arc::new(TimedSchedule::default());
    for tenant in ["first", "second", "third"] {
        schedule.write(&named(tenant), Some(due));
    }
    let manager = manager(&["first", "second", "third"], 1, HumanDuration::from_secs(60), due, Arc::clone(&schedule));

    let report = pass(&manager, now).await;

    assert_eq!(report.scheduled, 1, "one place, so one tenant is claimed");
    assert_eq!(
        report.overdue,
        overdue(2, due),
        "one tenant loaded, two left unclaimed behind its woken lifetime"
    );
    assert_eq!(report.overdue.map(|wakes| wakes.oldest_age_at(now)), Some(Duration::from_secs(30)));
}

#[tokio::test]
async fn a_pass_that_spends_its_whole_budget_counts_the_due_wakes_it_left_unclaimed() {
    let now = SystemTime::now();
    let names: Vec<String> = (1..=6).map(|index| format!("due{index}")).collect();
    let schedule = Arc::new(TimedSchedule::default());
    let mut instants = Vec::new();
    for (offset, name) in (1..=6).rev().zip(&names) {
        let at = now - Duration::from_secs(offset * 10);
        schedule.write(&named(name), Some(at));
        instants.push(at);
    }
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let manager = manager(&names, 64, HumanDuration::from_secs(60), now, Arc::clone(&schedule));

    let report = pass(&manager, now).await;

    assert_eq!(report.scheduled, 4, "the default budget of 4 is spent");
    assert_eq!(
        report.overdue,
        overdue(2, instants[4]),
        "the two latest wakes are left, the older of them due 20 s ago"
    );
}

#[tokio::test]
async fn a_pass_that_serves_every_due_wake_reports_none_overdue() {
    let now = SystemTime::now();
    let schedule = Arc::new(TimedSchedule::default());
    schedule.write(&named("first"), Some(now));
    let manager = manager(&["first"], 64, HumanDuration::from_secs(60), now, Arc::clone(&schedule));

    assert_eq!(pass(&manager, now).await.overdue, None);
}

#[tokio::test]
async fn a_wake_left_for_want_of_room_is_overdue_until_a_later_pass_loads_its_tenant() {
    let due = SystemTime::now() - Duration::from_secs(5);
    let schedule = Arc::new(TimedSchedule::default());
    schedule.write(&named("first"), Some(due));
    schedule.write(&named("second"), Some(due));
    let manager = manager(&["first", "second"], 1, HumanDuration::from_millis(300), due, Arc::clone(&schedule));

    let leaving = pass(&manager, SystemTime::now()).await;
    sleep(Duration::from_millis(400)).await;
    let serving = pass(&manager, SystemTime::now()).await;

    assert_eq!(leaving.overdue, overdue(1, due));
    assert_eq!((serving.scheduled, serving.overdue), (1, None), "the first woken lifetime ended and made room");
}
