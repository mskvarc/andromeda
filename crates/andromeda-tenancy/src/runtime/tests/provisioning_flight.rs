//! A new tenant's provisioning runs detached from the request that asked for
//! it. A request waits for it only until its admission deadline, the
//! provisioning runs on, and the request that retries joins it rather than
//! starting over. A provisioning not admitted by its deadline never runs, the
//! provisional sweep leaves a tenant whose provisioning is under way, and
//! finishing the slots cuts off the provisioning and sheds its waiters.

use crate::{
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    error::TenancyError,
    provisional::{Establishment, Retirement},
    registered_tenant::RegisteredTenant,
    runtime::{
        TenantRuntimeManager,
        provisioning_flight::ProvisioningSlot,
        tests::{FakeFactory, FakeProbe, FakeRegistry, FakeSchedule, GatedFactory, bounded_config, deadline_config, named, provisioning_slots},
    },
    saturated_wait::SaturatedWait,
    state_factory::TenantStateFactory,
    tenant_page_start::TenantPageStart,
    tenant_record::TenantRecord,
    tenant_ref::TenantRef,
};
use andromeda_configuration::{
    human_duration::HumanDuration,
    schema::tenancy::{TenancyConfig, TenantBuildConfig},
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_task_runtime::keyed_slots::KeyedSlots;
use async_trait::async_trait;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Notify, Semaphore},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

/// A registry whose provisioning records the tenant as provisional, then
/// holds until the test lets it end, as a migration on a loaded server does.
struct HeldProvisioning {
    /// The registry every other operation, and the recording, is answered by.
    inner: FakeRegistry,
    /// One permit per provisioning allowed to end.
    release: Semaphore,
    /// The failures the next provisionings to end answer with, the last one
    /// first; a provisioning finding none succeeds.
    failures: StdMutex<Vec<TenancyError>>,
}

impl HeldProvisioning {
    /// A registry of the established tenants `registered`, holding every
    /// provisioning.
    fn new(registered: &[&str]) -> HeldProvisioning {
        HeldProvisioning {
            inner: FakeRegistry::new(registered),
            release: Semaphore::new(0),
            failures: StdMutex::new(Vec::new()),
        }
    }

    /// Let `count` more provisionings end.
    fn release(&self, count: usize) {
        self.release.add_permits(count);
    }

    /// How many provisionings were started.
    fn provisioned(&self) -> usize {
        self.inner.provisioned.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TenantDatabase for HeldProvisioning {
    fn default_database(&self) -> TenantDatabaseName {
        self.inner.default_database()
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        self.inner.registered_tenants().await
    }

    async fn registered_tenant(&self, name: &TenantName) -> Result<Option<RegisteredTenant>, TenancyError> {
        self.inner.registered_tenant(name).await
    }

    async fn list_tenant_records(&self, start: &TenantPageStart, limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
        self.inner.list_tenant_records(start, limit).await
    }

    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        let database = self.inner.provision(name).await?;
        self.release.acquire().await.expect("the release semaphore stays open").forget();
        match self.failures.lock().expect("the failures lock is never poisoned").pop() {
            Some(failure) => Err(failure),
            None => Ok(database),
        }
    }

    async fn establish(&self, name: &TenantName) -> Result<Establishment, TenancyError> {
        self.inner.establish(name).await
    }

    async fn provisional_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        self.inner.provisional_tenants().await
    }

    async fn retire_provisional(&self, name: &TenantName, idle_for: Duration) -> Result<Retirement, TenancyError> {
        self.inner.retire_provisional(name, idle_for).await
    }
}

/// The manager the scenarios run.
type Manager = TenantRuntimeManager<HeldProvisioning, FakeFactory>;

/// A manager over `registry` whose admission deadline is 300 ms.
fn manager(registry: &Arc<HeldProvisioning>, factory: &Arc<FakeFactory>, slots: KeyedSlots<ProvisioningSlot>) -> Arc<Manager> {
    TenantRuntimeManager::new(
        deadline_config(64, 8, 8),
        Arc::clone(registry),
        Arc::clone(factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
        slots,
    )
}

fn factory() -> Arc<FakeFactory> {
    Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0))))
}

/// A request that may create `tenant`.
async fn create(manager: &Manager, tenant: &TenantRef) -> Result<String, TenancyError> {
    manager
        .resolve_state(tenant, TenantCreationPolicy::CreateOnDemand)
        .await
        .map(|leased| leased.state.as_str().to_owned())
}

/// Wait until the provisioning of `name` has ended.
async fn landed<F: TenantStateFactory>(manager: &TenantRuntimeManager<HeldProvisioning, F>, name: &TenantName) {
    timeout(Duration::from_secs(5), async {
        while manager.provisioning.is_in_flight(name) {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the provisioning ends");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_provisioning_outlasting_the_admission_deadline_sheds_the_request_and_runs_on_for_the_retry() {
    let registry = Arc::new(HeldProvisioning::new(&[]));
    let factory = factory();
    let manager = manager(&registry, &factory, provisioning_slots());
    let tenant = TenantRef::Named(named("fresh"));

    let shed = create(&manager, &tenant).await;
    assert!(
        matches!(
            shed,
            Err(TenancyError::Saturated {
                wait: SaturatedWait::Provisioning,
                ..
            })
        ),
        "{shed:?}"
    );
    assert!(manager.provisioning.is_in_flight(&named("fresh")), "the provisioning runs on past the request");

    let retry = {
        let manager = Arc::clone(&manager);
        let tenant = tenant.clone();
        tokio::spawn(async move { create(&manager, &tenant).await })
    };
    sleep(Duration::from_millis(50)).await;
    registry.release(1);
    let served = retry.await.expect("the retry joins");

    pretty_assertions::assert_eq!(served.ok().as_deref(), Some("db_fresh"));
    pretty_assertions::assert_eq!(registry.provisioned(), 1, "the retry joined the provisioning instead of starting another");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_waiting_for_a_failed_provisioning_reads_its_failure_and_the_next_request_provisions_again() {
    let registry = Arc::new(HeldProvisioning::new(&[]));
    registry
        .failures
        .lock()
        .expect("the failures lock is never poisoned")
        .push(TenancyError::ProvisioningFailed {
            name: named("fresh"),
            cause: PersistenceFailure::StatementRejected,
        });
    let factory = factory();
    let manager = manager(&registry, &factory, provisioning_slots());
    let tenant = TenantRef::Named(named("fresh"));
    let first = create(&manager, &tenant).await;
    assert!(matches!(first, Err(TenancyError::Saturated { .. })), "{first:?}");

    let joined = {
        let manager = Arc::clone(&manager);
        let tenant = tenant.clone();
        tokio::spawn(async move { create(&manager, &tenant).await })
    };
    sleep(Duration::from_millis(50)).await;
    registry.release(1);
    let failed = joined.await.expect("the joined request ends");
    assert!(
        matches!(
            failed,
            Err(TenancyError::ProvisioningFailed {
                cause: PersistenceFailure::StatementRejected,
                ..
            })
        ),
        "{failed:?}"
    );
    landed(&manager, &named("fresh")).await;

    registry.release(1);
    let served = create(&manager, &tenant).await;

    pretty_assertions::assert_eq!(served.ok().as_deref(), Some("db_fresh"));
    pretty_assertions::assert_eq!(registry.provisioned(), 2, "a failed provisioning is not reused");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_provisioning_not_admitted_by_its_deadline_never_runs() {
    // The one build permit is held by a slow build of an established tenant,
    // so the new tenant's provisioning cannot start before its deadline.
    let registry = Arc::new(HeldProvisioning::new(&["slow"]));
    registry.release(8);
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::new(AtomicUsize::new(0)),
        gate: Arc::clone(&gate),
    });
    let manager = TenantRuntimeManager::new(
        deadline_config(64, 1, 8),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
        provisioning_slots(),
    );
    let slow = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            manager
                .resolve_state(&TenantRef::Named(named("slow")), TenantCreationPolicy::MustExist)
                .await
                .map(|leased| leased.state)
        })
    };
    sleep(Duration::from_millis(50)).await;
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 1);

    let refused = manager
        .resolve_state(&TenantRef::Named(named("fresh")), TenantCreationPolicy::CreateOnDemand)
        .await;
    assert!(matches!(refused, Err(TenancyError::Saturated { .. })), "{:?}", refused.err());
    gate.notify_waiters();
    assert!(slow.await.expect("the slow build joins").is_ok());
    landed(&manager, &named("fresh")).await;

    pretty_assertions::assert_eq!(registry.provisioned(), 0, "a provisioning past its deadline is dropped unstarted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_provisional_sweep_leaves_a_tenant_whose_provisioning_is_under_way() {
    let registry = Arc::new(HeldProvisioning::new(&[]));
    let factory = factory();
    let manager = manager(&registry, &factory, provisioning_slots());
    let shed = create(&manager, &TenantRef::Named(named("fresh"))).await;
    assert!(matches!(shed, Err(TenancyError::Saturated { .. })), "{shed:?}");

    let during = manager.settle_provisional().await;
    pretty_assertions::assert_eq!(
        registry.inner.offered(),
        Vec::<String>::new(),
        "a migrating tenant is not offered for retirement"
    );
    pretty_assertions::assert_eq!(registry.inner.establishments.load(Ordering::SeqCst), 0, "a migrating tenant is not probed");

    registry.release(1);
    landed(&manager, &named("fresh")).await;
    let after = manager.settle_provisional().await;

    pretty_assertions::assert_eq!(during.retired + during.established, 0);
    pretty_assertions::assert_eq!(registry.inner.offered(), vec!["fresh".to_owned()], "a provisioned tenant is settled again");
    pretty_assertions::assert_eq!(after.retired, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finishing_the_provisioning_slots_cuts_off_the_provisioning_and_sheds_its_waiter_at_once() {
    let registry = Arc::new(HeldProvisioning::new(&[]));
    let factory = factory();
    let bounded = bounded_config(64, 8, 8);
    let patient = TenancyConfig {
        build: TenantBuildConfig {
            admission_timeout: HumanDuration::from_secs(30),
            ..bounded.build
        },
        ..bounded
    };
    let slots = KeyedSlots::new(NonZeroUsize::MIN.saturating_add(7), CancellationToken::new());
    let manager = TenantRuntimeManager::new(
        patient,
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
        slots.clone(),
    );
    let waiting = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move { create(&manager, &TenantRef::Named(named("fresh"))).await })
    };
    sleep(Duration::from_millis(50)).await;
    pretty_assertions::assert_eq!(registry.provisioned(), 1);

    let finishing = Instant::now();
    timeout(Duration::from_secs(5), slots.finish()).await.expect("the slots finish");
    let shed = timeout(Duration::from_secs(5), waiting)
        .await
        .expect("the waiter is answered")
        .expect("the waiter joins");

    assert!(
        matches!(
            shed,
            Err(TenancyError::Saturated {
                wait: SaturatedWait::Provisioning,
                ..
            })
        ),
        "{shed:?}"
    );
    assert!(
        finishing.elapsed() < Duration::from_secs(2),
        "the waiter is shed when the provisioning is cut off, not at its deadline"
    );
    assert!(!manager.provisioning.is_in_flight(&named("fresh")), "a cut-off provisioning leaves the flights");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 0);
}
