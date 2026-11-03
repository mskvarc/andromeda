//! Which runtime is unloaded to make room when some hold changes they have
//! accepted and not yet acted on.

use crate::{
    creation_policy::TenantCreationPolicy,
    runtime::{
        TenantRuntimeManager,
        lease::LeasedState,
        tests::{FakeFactory, FakeRegistry, Harness, harness, named},
    },
    tenant_ref::TenantRef,
};
use std::{sync::atomic::Ordering, time::Duration};
use tokio::time::sleep;

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

/// Whether `name` still has a runtime in the cache.
fn resident(manager: &TenantRuntimeManager<FakeRegistry, FakeFactory>, name: &str) -> bool {
    manager.runtimes.contains_key(&tenant(name))
}

/// X is used before Y, so by last use alone X is the victim.
#[tokio::test]
async fn a_cold_build_unloads_a_runtime_holding_nothing_before_an_older_one_holding_changes() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y", "z"], true, 2);
    drop(resolve(&manager, "x").await);
    drop(resolve(&manager, "y").await);
    factory.hold_changes(&tenant("x"), 4);

    let _z = resolve(&manager, "z").await;
    manager.await_evictions().await;

    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    assert!(resident(&manager, "x"), "the runtime holding changes is kept");
    assert!(!resident(&manager, "y"), "the runtime holding nothing is unloaded");
}

/// `max_resident` stays a hard bound: when every candidate holds changes, the
/// least recently used of them is unloaded.
#[tokio::test]
async fn when_every_candidate_holds_changes_the_least_recently_used_is_unloaded() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y", "z"], true, 2);
    drop(resolve(&manager, "x").await);
    drop(resolve(&manager, "y").await);
    factory.hold_changes(&tenant("x"), 1);
    factory.hold_changes(&tenant("y"), 1);

    let _z = resolve(&manager, "z").await;
    manager.await_evictions().await;

    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    pretty_assertions::assert_eq!(manager.runtimes.len(), 2, "the cache stays at max_resident");
    assert!(!resident(&manager, "x"), "the least recently used runtime is unloaded");
    assert!(resident(&manager, "y"));
}

#[tokio::test]
async fn an_idle_runtime_holding_changes_is_not_swept() {
    let Harness {
        manager, factory, shutdowns, ..
    } = harness(&["x", "y"], true, 4);
    drop(resolve(&manager, "x").await);
    drop(resolve(&manager, "y").await);
    factory.hold_changes(&tenant("x"), 2);
    sleep(Duration::from_millis(20)).await;

    manager.sweep_idle_dormant(Duration::from_millis(5)).await;
    manager.await_evictions().await;

    assert!(resident(&manager, "x"), "the runtime holding changes is in use until they are acted on");
    assert!(!resident(&manager, "y"), "the idle runtime holding nothing is swept");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}
