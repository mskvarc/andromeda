//! How one wake pass shares its budget between the tenants its schedule holds
//! due and the tenants behind the server's extension versions.
//!
//! Every scenario runs at the default `max_wakes_per_pass` of 4.

use crate::{
    catch_up::{CatchUpCandidates, CatchUpError},
    creation_policy::TenantCreationPolicy,
    runtime::{
        TenantRuntimeManager,
        tests::{FakeFactory, FakeProbe, FakeRegistry, FakeSchedule, config, named},
        wake_pass::{NextWakePass, WakePassReport},
    },
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;
use pretty_assertions::assert_eq;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

/// A catch-up source listing fixed tenants, which counts its reads.
pub(super) struct FakeCatchUp {
    /// The tenants listed, furthest behind first.
    lagging: Vec<String>,
    /// How many times the list was read.
    reads: AtomicUsize,
}

impl FakeCatchUp {
    /// A source listing `lagging` in that order.
    pub(super) fn listing(lagging: &[&str]) -> FakeCatchUp {
        FakeCatchUp {
            lagging: lagging.iter().map(|name| (*name).to_string()).collect(),
            reads: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl CatchUpCandidates for FakeCatchUp {
    async fn lagging_tenants(&self) -> Result<Vec<TenantName>, CatchUpError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(self.lagging.iter().map(|name| named(name)).collect())
    }
}

/// The manager under test, its registry and its factory.
struct PassHarness {
    /// The manager running the pass.
    manager: Arc<TenantRuntimeManager<FakeRegistry, FakeFactory>>,
    /// The registry it resolves against.
    registry: Arc<FakeRegistry>,
    /// The factory it builds through.
    factory: Arc<FakeFactory>,
}

/// A manager over `registered` tenants whose schedule holds `due` due and
/// whose probe finds every claimed tenant's work due as it answers.
fn pass_harness(registered: &[&str], max_cached: usize, due: &[&str]) -> PassHarness {
    let registry = Arc::new(FakeRegistry::new(registered));
    let factory = Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0))));
    let manager = TenantRuntimeManager::new(
        config(true, max_cached),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::due_as_answered()),
        Arc::new(FakeSchedule::claiming(due)),
    );
    PassHarness { manager, registry, factory }
}

/// Run one pass against `catch_up`.
async fn run(manager: &TenantRuntimeManager<FakeRegistry, FakeFactory>, catch_up: &FakeCatchUp) -> WakePassReport {
    let now = SystemTime::now();
    manager
        .run_wake_pass(catch_up, now, now + Duration::from_mins(1))
        .await
        .expect("the pass completes")
}

/// Whether `name` has a runtime in the cache.
fn resident(manager: &TenantRuntimeManager<FakeRegistry, FakeFactory>, name: &str) -> bool {
    manager.runtimes.contains_key(&TenantRef::Named(named(name)))
}

#[tokio::test]
async fn the_catch_up_loads_only_what_the_scheduled_wakes_leave_of_the_pass_budget() {
    let PassHarness { manager, .. } = pass_harness(&["due1", "due2", "due3", "lag1", "lag2"], 64, &["due1", "due2", "due3"]);
    let catch_up = FakeCatchUp::listing(&["lag1", "lag2"]);

    let report = run(&manager, &catch_up).await;

    assert_eq!(
        report,
        WakePassReport {
            scheduled: 3,
            caught_up: 1,
            overdue: None,
            next: NextWakePass::WhenRecorded,
        }
    );
    assert!(
        ["due1", "due2", "due3"].iter().all(|name| resident(&manager, name)),
        "every due tenant is woken"
    );
    assert!(resident(&manager, "lag1"), "the furthest-behind tenant takes the one slot left");
    assert!(!resident(&manager, "lag2"), "no tenant is loaded past the pass budget");
}

#[tokio::test]
async fn a_pass_whose_scheduled_wakes_fill_the_budget_reads_no_catch_up_candidates() {
    let PassHarness { manager, .. } = pass_harness(&["due1", "due2", "due3", "due4", "lag1"], 64, &["due1", "due2", "due3", "due4"]);
    let catch_up = FakeCatchUp::listing(&["lag1"]);

    let report = run(&manager, &catch_up).await;

    assert_eq!(
        report,
        WakePassReport {
            scheduled: 4,
            caught_up: 0,
            overdue: None,
            next: NextWakePass::Immediately,
        }
    );
    assert_eq!(catch_up.reads.load(Ordering::Relaxed), 0);
    assert!(!resident(&manager, "lag1"));
}

#[tokio::test]
async fn the_catch_up_loads_the_candidates_in_the_order_they_are_listed() {
    let PassHarness { manager, .. } = pass_harness(&["due1", "due2", "a", "b", "c"], 64, &["due1", "due2"]);
    let catch_up = FakeCatchUp::listing(&["c", "a", "b"]);

    let report = run(&manager, &catch_up).await;

    assert_eq!(
        report,
        WakePassReport {
            scheduled: 2,
            caught_up: 2,
            overdue: None,
            next: NextWakePass::WhenRecorded,
        }
    );
    assert!(resident(&manager, "c") && resident(&manager, "a"));
    assert!(!resident(&manager, "b"), "the third candidate waits for a later pass");
}

#[tokio::test]
async fn the_catch_up_passes_over_a_resident_tenant_without_spending_budget() {
    let PassHarness { manager, factory, .. } = pass_harness(&["lag1", "lag2"], 64, &[]);
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("lag1")), TenantCreationPolicy::MustExist)
            .await
            .expect("the tenant resolves"),
    );
    let catch_up = FakeCatchUp::listing(&["lag1", "lag2"]);

    let report = run(&manager, &catch_up).await;

    assert_eq!(
        report,
        WakePassReport {
            scheduled: 0,
            caught_up: 1,
            overdue: None,
            next: NextWakePass::WhenRecorded,
        }
    );
    assert!(resident(&manager, "lag2"));
    assert_eq!(factory.built.load(Ordering::Relaxed), 2, "the resident tenant is not built again");
}

#[tokio::test]
async fn the_catch_up_unloads_no_tenant_to_make_room() {
    let PassHarness { manager, factory, .. } = pass_harness(&["busy", "lag1"], 1, &[]);
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("busy")), TenantCreationPolicy::MustExist)
            .await
            .expect("the tenant resolves"),
    );
    let catch_up = FakeCatchUp::listing(&["lag1"]);

    let report = run(&manager, &catch_up).await;

    assert_eq!(
        report,
        WakePassReport {
            scheduled: 0,
            caught_up: 0,
            overdue: None,
            next: NextWakePass::WhenRecorded,
        }
    );
    assert!(resident(&manager, "busy"), "the request-loaded tenant keeps its slot");
    assert!(!resident(&manager, "lag1"));
    assert_eq!(factory.built.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn the_catch_up_passes_over_a_suspended_tenant() {
    let PassHarness { manager, registry, .. } = pass_harness(&["lag1", "lag2"], 64, &[]);
    registry.suspend("lag1");
    let catch_up = FakeCatchUp::listing(&["lag1", "lag2"]);

    let report = run(&manager, &catch_up).await;

    assert_eq!(
        report,
        WakePassReport {
            scheduled: 0,
            caught_up: 1,
            overdue: None,
            next: NextWakePass::WhenRecorded,
        }
    );
    assert!(!resident(&manager, "lag1"), "a suspended tenant is never woken");
    assert!(resident(&manager, "lag2"));
}
