//! Which runtime is unloaded to make room when some have a periodic
//! notification falling due before an unloaded tenant could be woken again.
//!
//! Every scenario runs at the default `min_wake_interval` of 30 s, so a
//! notification due in 1 s is imminent and one due in 60 s is not.

use crate::{
    creation_policy::TenantCreationPolicy,
    runtime::{
        TenantRuntimeManager,
        lease::LeasedState,
        tests::{FakeFactory, FakeRegistry, Harness, harness, named},
    },
    tenant_ref::TenantRef,
};
use std::{
    sync::atomic::Ordering,
    time::{Duration, SystemTime},
};
use tokio::time::sleep;

/// Within `min_wake_interval`: unloaded now, the tenant would notify late.
const IMMINENT: Duration = Duration::from_secs(1);

/// Past `min_wake_interval`: unloaded now, the tenant is woken in time.
const DISTANT: Duration = Duration::from_mins(1);

fn tenant(name: &str) -> TenantRef {
    TenantRef::Named(named(name))
}

/// Resolve `name`, then wait long enough that the next resolve stamps a later
/// last use.
async fn resolve(manager: &TenantRuntimeManager<FakeRegistry, FakeFactory>, name: &str) -> LeasedState<String> {
    let leased = manager
        .resolve_state(&tenant(name), TenantCreationPolicy::MustExist)
        .await
        .expect("the tenant resolves");
    sleep(Duration::from_millis(3)).await;
    leased
}

/// Have `name`'s runtime report its next periodic notification `due_in` from
/// now.
fn schedule(factory: &FakeFactory, name: &str, due_in: Duration) {
    factory.schedule_periodic(&tenant(name), SystemTime::now() + due_in);
}

/// Whether `name` still has a runtime in the cache.
fn resident(manager: &TenantRuntimeManager<FakeRegistry, FakeFactory>, name: &str) -> bool {
    manager.runtimes.contains_key(&tenant(name))
}

/// X is used before Y, so by last use alone X is the victim; its sweep then
/// publishes a notification due in 1 s, as a sweep does after its first pass.
#[tokio::test]
async fn a_cold_build_unloads_a_runtime_with_nothing_due_before_an_older_one_with_a_periodic_notification_due() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y", "z"], true, 2);
    drop(resolve(&manager, "x").await);
    drop(resolve(&manager, "y").await);
    schedule(&factory, "x", IMMINENT);

    let _z = resolve(&manager, "z").await;
    manager.await_evictions().await;

    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    assert!(resident(&manager, "x"), "the runtime with a periodic notification due is kept");
    assert!(!resident(&manager, "y"), "the runtime with nothing due is unloaded");
}

/// The cold build finds every runtime leased and overshoots the cap, so the
/// room is made by the trim that runs once the leases drop.
#[tokio::test]
async fn the_excess_trim_unloads_a_runtime_with_nothing_due_before_an_older_one_with_a_periodic_notification_due() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y", "z"], true, 2);
    let x = resolve(&manager, "x").await;
    let y = resolve(&manager, "y").await;
    schedule(&factory, "x", IMMINENT);
    let _z = resolve(&manager, "z").await;
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(manager.runtimes.len(), 3, "every runtime was leased, so the cache overshot");
    drop(x);
    drop(y);

    pretty_assertions::assert_eq!(manager.trim_excess_runtimes(None).await, 1);
    manager.await_evictions().await;

    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    assert!(resident(&manager, "x"), "the runtime with a periodic notification due is kept");
    assert!(!resident(&manager, "y"), "the runtime with nothing due is unloaded");
}

#[tokio::test]
async fn a_periodic_notification_due_after_the_minimum_wake_interval_leaves_the_order_to_last_use() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y", "z"], true, 2);
    drop(resolve(&manager, "x").await);
    drop(resolve(&manager, "y").await);
    schedule(&factory, "x", DISTANT);

    let _z = resolve(&manager, "z").await;
    manager.await_evictions().await;

    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    assert!(!resident(&manager, "x"), "the least recently used runtime is unloaded");
    assert!(resident(&manager, "y"));
}

/// `max_resident` stays a hard bound: when every candidate has a periodic
/// notification due, the least recently used of them is unloaded and notifies
/// late, rather than the cache growing past the bound.
#[tokio::test]
async fn when_every_candidate_has_a_periodic_notification_due_the_least_recently_used_is_unloaded() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y", "z"], true, 2);
    drop(resolve(&manager, "x").await);
    drop(resolve(&manager, "y").await);
    schedule(&factory, "x", IMMINENT);
    schedule(&factory, "y", IMMINENT);

    let _z = resolve(&manager, "z").await;
    manager.await_evictions().await;

    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    pretty_assertions::assert_eq!(manager.runtimes.len(), 2, "the cache stays at max_resident");
    assert!(!resident(&manager, "x"), "the least recently used runtime is unloaded");
    assert!(resident(&manager, "y"));
}
