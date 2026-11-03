//! Unit tests for the tenant runtime cache: single-flight cold builds, the
//! lease that spares an in-use runtime, and the orphan, excess and idle sweeps.

use crate::{
    administration::TenantAdministration,
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    error::TenancyError,
    lifecycle::TenantLifecycle,
    pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
    provisional::{Establishment, Retirement},
    registered_tenant::RegisteredTenant,
    runtime::{TenantRuntimeManager, provisional_sweep::ProvisionalSettlement},
    runtime_build_failure::RuntimeBuildFailure,
    saturated_wait::SaturatedWait,
    state_factory::{RuntimeActivity, TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_existence::TenantExistence,
    tenant_page_start::TenantPageStart,
    tenant_record::TenantRecord,
    tenant_ref::TenantRef,
    wake_backlog::OverdueWakes,
    wake_schedule::{TenantWakeSchedule, WakeScheduleError},
};
use andromeda_configuration::{
    human_duration::HumanDuration,
    schema::{
        feature_mode::FeatureMode,
        tenancy::{ResidencyConfig, TenancyConfig, TenantBuildConfig},
    },
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_telemetry::tenancy::{EvictionReason, WakeOutcome};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    sync::{Notify, Semaphore},
    time::{sleep, timeout},
};

mod backlog_eviction;
mod overdue_wakes;
mod periodic_eviction;
mod wake_pass;
mod wake_timing;

/// What a [`FakeProbe`] answers with for every database.
enum FakeProbeOutcome {
    /// A pending-work reading.
    Work(PendingWork),
    /// Work due at the clock reading the probe takes while it answers, as a
    /// database source due at once reports it.
    DueAsAnswered,
    /// The tenant's database is gone (SQLSTATE 3D000 at the store).
    DatabaseGone,
    /// The tenant's database holds no schema of this broker's release yet.
    Unmigrated,
}

/// A probe that answers every tenant with one fixed outcome.
struct FakeProbe {
    /// The outcome returned for every database.
    outcome: FakeProbeOutcome,
    /// The standing of the tenant each probe was asked about, in order.
    asked: StdMutex<Vec<TenantExistence>>,
}

impl FakeProbe {
    /// A probe reporting nothing due.
    fn none() -> FakeProbe {
        FakeProbe {
            outcome: FakeProbeOutcome::Work(PendingWork::None),
            asked: StdMutex::new(Vec::new()),
        }
    }

    /// A probe reporting work due at `due`.
    fn due_at(due: SystemTime) -> FakeProbe {
        FakeProbe {
            outcome: FakeProbeOutcome::Work(PendingWork::DueAt(due)),
            asked: StdMutex::new(Vec::new()),
        }
    }

    /// A probe reporting work due at the instant it answers.
    fn due_as_answered() -> FakeProbe {
        FakeProbe {
            outcome: FakeProbeOutcome::DueAsAnswered,
            asked: StdMutex::new(Vec::new()),
        }
    }

    /// A probe whose tenant database no longer exists.
    fn database_gone() -> FakeProbe {
        FakeProbe {
            outcome: FakeProbeOutcome::DatabaseGone,
            asked: StdMutex::new(Vec::new()),
        }
    }

    /// A probe whose tenant database is not migrated.
    fn unmigrated() -> FakeProbe {
        FakeProbe {
            outcome: FakeProbeOutcome::Unmigrated,
            asked: StdMutex::new(Vec::new()),
        }
    }

    /// The standing of the tenant each probe was asked about, in order.
    fn asked(&self) -> Vec<TenantExistence> {
        self.asked.lock().map(|asked| asked.clone()).unwrap_or_default()
    }
}

#[async_trait]
impl PendingWorkProbe for FakeProbe {
    async fn earliest_pending_work(&self, database: &TenantDatabaseName, existence: TenantExistence) -> Result<PendingWork, PendingWorkError> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(existence);
        }
        match &self.outcome {
            FakeProbeOutcome::Work(work) => Ok(*work),
            FakeProbeOutcome::DueAsAnswered => {
                sleep(Duration::from_millis(1)).await;
                Ok(PendingWork::DueAt(SystemTime::now()))
            }
            FakeProbeOutcome::DatabaseGone => Err(PendingWorkError::DatabaseGone { database: database.clone() }), // clone: the error outlives this borrow
            FakeProbeOutcome::Unmigrated => Err(PendingWorkError::Unmigrated { database: database.clone() }),     // clone: the error outlives this borrow
        }
    }
}

/// A wake schedule that records its writes and hands back configured claims.
struct FakeSchedule {
    /// Every `record` call's tenant and next wake.
    recorded: StdMutex<Vec<(String, Option<SystemTime>)>>,
    /// Every `settle` call's tenant and next wake.
    settled: StdMutex<Vec<(String, Option<SystemTime>)>>,
    /// The tenants held due, handed out by `claim_due` up to its limit.
    due: StdMutex<Vec<String>>,
    /// What `earliest_wake` answers.
    earliest: StdMutex<Option<SystemTime>>,
}

impl FakeSchedule {
    /// A schedule that records writes and claims nothing.
    fn inert() -> FakeSchedule {
        FakeSchedule::claiming(&[])
    }

    /// A schedule that records writes and holds `due` due, in that order.
    fn claiming(due: &[&str]) -> FakeSchedule {
        FakeSchedule {
            recorded: StdMutex::new(Vec::new()),
            settled: StdMutex::new(Vec::new()),
            due: StdMutex::new(due.iter().map(|name| (*name).to_string()).collect()),
            earliest: StdMutex::new(None),
        }
    }

    /// Hold `earliest` as the earliest wake of any tenant.
    fn hold_earliest(&self, earliest: Option<SystemTime>) {
        if let Ok(mut held) = self.earliest.lock() {
            *held = earliest;
        }
    }

    /// How many times `settle` was called.
    fn settle_count(&self) -> usize {
        self.settled.lock().map(|settled| settled.len()).unwrap_or_default()
    }

    /// Every `record` call's tenant name and recorded wake instant.
    fn recorded(&self) -> Vec<(String, Option<SystemTime>)> {
        self.recorded.lock().map(|recorded| recorded.clone()).unwrap_or_default()
    }
}

#[async_trait]
impl TenantWakeSchedule for FakeSchedule {
    async fn record(&self, tenant: &TenantName, _probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        if let Ok(mut recorded) = self.recorded.lock() {
            recorded.push((tenant.as_str().to_string(), wake_at));
        }
        Ok(())
    }

    async fn claim_due(&self, _now: SystemTime, _lease_until: SystemTime, limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        let claimed: Vec<String> = self
            .due
            .lock()
            .map(|mut due| {
                let take = limit.min(due.len());
                due.drain(..take).collect()
            })
            .unwrap_or_default();
        Ok(claimed.into_iter().filter_map(|name| TenantName::try_from(name).ok()).collect())
    }

    async fn earliest_wake(&self) -> Result<Option<SystemTime>, WakeScheduleError> {
        Ok(self.earliest.lock().map(|earliest| *earliest).unwrap_or_default())
    }

    async fn overdue(&self, _now: SystemTime) -> Result<Option<OverdueWakes>, WakeScheduleError> {
        Ok(None)
    }

    async fn settle(&self, tenant: &TenantName, _probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        if let Ok(mut settled) = self.settled.lock() {
            settled.push((tenant.as_str().to_string(), wake_at));
        }
        Ok(())
    }

    async fn next_unprobed(&self, _limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        Ok(Vec::new())
    }
}

fn named(raw: &str) -> TenantName {
    TenantName::try_from(raw.to_string()).expect("a valid tenant name")
}

fn config(enabled: bool, max_cached: usize) -> TenancyConfig {
    TenancyConfig {
        mode: if enabled { FeatureMode::Enabled } else { FeatureMode::Disabled },
        residency: ResidencyConfig {
            max_resident: NonZeroUsize::new(max_cached).expect("a positive cache size"),
            ..ResidencyConfig::default()
        },
        ..TenancyConfig::default()
    }
}

/// A config with an explicit build-admission and eviction bound, for the tests
/// that assert those bounds are honoured.
fn bounded_config(max_cached: usize, max_concurrent_builds: usize, eviction_concurrency: usize) -> TenancyConfig {
    let base = config(true, max_cached);
    TenancyConfig {
        residency: ResidencyConfig {
            max_concurrent_evictions: NonZeroUsize::new(eviction_concurrency).expect("a positive eviction bound"),
            ..base.residency
        },
        build: TenantBuildConfig {
            max_concurrent: NonZeroUsize::new(max_concurrent_builds).expect("a positive build bound"),
            ..base.build
        },
        ..base
    }
}

/// A registry holding a mutable set of tenants, which records provisioning.
struct FakeRegistry {
    /// Tenants that are established.
    registered: StdMutex<Vec<String>>,
    /// Tenants registered for a creating operation that do not exist yet.
    provisional: StdMutex<Vec<String>>,
    /// Tenants whose database holds information.
    occupied: StdMutex<Vec<String>>,
    /// Registered tenants whose administrative state is suspended.
    suspended: StdMutex<Vec<String>>,
    /// How many times `provision` was called.
    provisioned: AtomicUsize,
    /// How many times `establish` was called.
    establishments: AtomicUsize,
    /// The names `retire_provisional` was offered, in order.
    offered: StdMutex<Vec<String>>,
}

/// A fixed timestamp for a fake registry row, since these tests never assert on
/// tenant timestamps.
fn epoch() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).expect("the Unix epoch is representable")
}

impl FakeRegistry {
    /// A registry holding the given tenants.
    fn new(names: &[&str]) -> FakeRegistry {
        FakeRegistry {
            registered: StdMutex::new(names.iter().map(|name| (*name).to_string()).collect()),
            provisional: StdMutex::new(Vec::new()),
            occupied: StdMutex::new(Vec::new()),
            suspended: StdMutex::new(Vec::new()),
            provisioned: AtomicUsize::new(0),
            establishments: AtomicUsize::new(0),
            offered: StdMutex::new(Vec::new()),
        }
    }

    /// Every provisional name.
    fn provisional(&self) -> Vec<String> {
        self.provisional.lock().map(|names| names.clone()).unwrap_or_default()
    }

    /// Whether a tenant is provisional.
    fn is_provisional(&self, name: &str) -> bool {
        self.provisional().iter().any(|held| held == name)
    }

    /// Register a tenant as provisional, as an interrupted or refused creating
    /// operation leaves it.
    fn register_provisional(&self, name: &str) {
        if let Ok(mut names) = self.provisional.lock() {
            names.push(name.to_string());
        }
    }

    /// Record that a tenant's database now holds information, as a committed
    /// creating operation leaves it.
    fn occupy(&self, name: &str) {
        if let Ok(mut names) = self.occupied.lock() {
            names.push(name.to_string());
        }
    }

    /// Whether a tenant's database holds information.
    fn is_occupied(&self, name: &str) -> bool {
        self.occupied.lock().is_ok_and(|names| names.iter().any(|held| held == name))
    }

    /// Move a provisional tenant to the established tenants.
    fn promote(&self, name: &str) {
        if let Ok(mut names) = self.provisional.lock() {
            names.retain(|held| held != name);
        }
        if let Ok(mut names) = self.registered.lock() {
            names.push(name.to_string());
        }
    }

    /// The names offered for retirement so far.
    fn offered(&self) -> Vec<String> {
        self.offered.lock().map(|names| names.clone()).unwrap_or_default()
    }

    /// Every registered name.
    fn all(&self) -> Vec<String> {
        self.registered.lock().map(|names| names.clone()).unwrap_or_default()
    }

    /// Forget a tenant, as an operator dropping its row would.
    fn forget(&self, name: &str) {
        if let Ok(mut names) = self.registered.lock() {
            names.retain(|held| held != name);
        }
    }

    /// Mark a registered tenant as administratively suspended.
    fn suspend(&self, name: &str) {
        if let Ok(mut names) = self.suspended.lock() {
            names.push(name.to_string());
        }
    }

    /// Return a suspended tenant to service.
    fn resume(&self, name: &str) {
        if let Ok(mut names) = self.suspended.lock() {
            names.retain(|held| held != name);
        }
    }

    /// Whether a tenant is administratively suspended.
    fn is_suspended(&self, name: &str) -> bool {
        self.suspended.lock().is_ok_and(|names| names.iter().any(|held| held == name))
    }

    /// The registry row a registered tenant projects.
    fn record_for(&self, name: &TenantName) -> TenantRecord {
        let status = if self.is_suspended(name.as_str()) {
            TenantStatus::Suspended
        } else {
            TenantStatus::Active
        };
        TenantRecord::new(
            name.clone(), // clone: the record owns its name
            status,
            TenantDatabaseName::from(format!("db_{}", name.as_str())),
            epoch(),
            epoch(),
        )
    }
}

#[async_trait]
impl TenantDatabase for FakeRegistry {
    fn default_database(&self) -> TenantDatabaseName {
        TenantDatabaseName::from("andromeda".to_string())
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        Ok(self.all().into_iter().filter_map(|name| TenantName::try_from(name).ok()).collect())
    }

    async fn registered_tenant(&self, name: &TenantName) -> Result<Option<RegisteredTenant>, TenancyError> {
        if self.is_provisional(name.as_str()) {
            return Ok(Some(RegisteredTenant::Provisional {
                database: TenantDatabaseName::from(format!("db_{}", name.as_str())),
            }));
        }
        Ok(self
            .all()
            .iter()
            .any(|held| held == name.as_str())
            .then(|| RegisteredTenant::Established(self.record_for(name))))
    }

    async fn list_tenant_records(&self, start: &TenantPageStart, limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
        let mut names = self.all();
        names.sort();
        let total = i64::try_from(names.len()).unwrap_or(i64::MAX);
        let window = usize::try_from(limit).unwrap_or(usize::MAX);
        let selected: Vec<String> = match start {
            TenantPageStart::Offset(offset) => names.into_iter().skip(usize::try_from(*offset).unwrap_or(usize::MAX)).take(window).collect(),
            TenantPageStart::Forward { after } => names
                .into_iter()
                .filter(|held| after.as_ref().is_none_or(|bound| held.as_str() > bound.as_str()))
                .take(window)
                .collect(),
            TenantPageStart::Backward { before } => {
                let below: Vec<String> = names.into_iter().filter(|held| held.as_str() < before.as_str()).collect();
                let start = below.len().saturating_sub(window);
                below.into_iter().skip(start).collect()
            }
            TenantPageStart::FromEnd => {
                let start = names.len().saturating_sub(window);
                names.into_iter().skip(start).collect()
            }
        };
        let records = selected
            .into_iter()
            .filter_map(|name| TenantName::try_from(name).ok().map(|name| self.record_for(&name)))
            .collect();
        Ok((records, total))
    }

    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        self.provisioned.fetch_add(1, Ordering::Relaxed);
        let known = self.is_provisional(name.as_str()) || self.all().iter().any(|held| held == name.as_str());
        if !known && let Ok(mut names) = self.provisional.lock() {
            names.push(name.as_str().to_string());
        }
        Ok(TenantDatabaseName::from(format!("db_{}", name.as_str())))
    }

    async fn establish(&self, name: &TenantName) -> Result<Establishment, TenancyError> {
        self.establishments.fetch_add(1, Ordering::Relaxed);
        if self.is_provisional(name.as_str()) {
            if !self.is_occupied(name.as_str()) {
                return Ok(Establishment::Empty);
            }
            self.promote(name.as_str());
        }
        Ok(if self.all().iter().any(|held| held == name.as_str()) {
            Establishment::Established
        } else {
            Establishment::Empty
        })
    }

    async fn provisional_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        Ok(self.provisional().into_iter().filter_map(|name| TenantName::try_from(name).ok()).collect())
    }

    async fn retire_provisional(&self, name: &TenantName, _idle_for: Duration) -> Result<Retirement, TenancyError> {
        if let Ok(mut offered) = self.offered.lock() {
            offered.push(name.as_str().to_string());
        }
        if !self.is_provisional(name.as_str()) {
            return Ok(Retirement::Kept);
        }
        if self.is_occupied(name.as_str()) {
            self.promote(name.as_str());
            return Ok(Retirement::Established);
        }
        if let Ok(mut names) = self.provisional.lock() {
            names.retain(|held| held != name.as_str());
        }
        Ok(Retirement::Retired)
    }
}

#[async_trait]
impl TenantLifecycle for FakeRegistry {
    async fn set_status(&self, name: &TenantName, status: TenantStatus) -> Result<(), TenancyError> {
        match status {
            TenantStatus::Suspended => self.suspend(name.as_str()),
            TenantStatus::Active => self.resume(name.as_str()),
        }
        Ok(())
    }
}

/// Where the test runtimes report use: nowhere, since none paces background work.
struct Unobserved;

impl RuntimeActivity for Unobserved {
    fn record_activity(&self) {}
}

/// The instant a fake runtime reports its next periodic notification due at,
/// settable after the runtime is built, as a periodic sweep publishes it.
type PeriodicDueCell = Arc<StdMutex<Option<SystemTime>>>;

/// How many changes a fake runtime reports holding, settable after the runtime
/// is built, as its queue fills and drains.
type BacklogCell = Arc<AtomicUsize>;

/// A runtime that records the fact it was released.
struct FakeRuntime {
    /// Incremented on shutdown.
    shutdowns: Arc<AtomicUsize>,
    /// What [`TenantRuntime::next_periodic_due`] answers.
    periodic_due: PeriodicDueCell,
    /// What [`TenantRuntime::undelivered_changes`] answers.
    backlog: BacklogCell,
}

impl FakeRuntime {
    /// A runtime with no periodic notification scheduled.
    fn unscheduled(shutdowns: &Arc<AtomicUsize>) -> FakeRuntime {
        FakeRuntime {
            shutdowns: Arc::clone(shutdowns),
            periodic_due: PeriodicDueCell::default(),
            backlog: BacklogCell::default(),
        }
    }
}

#[async_trait]
impl TenantRuntime for FakeRuntime {
    async fn shutdown(self) {
        self.shutdowns.fetch_add(1, Ordering::Relaxed);
    }

    fn activity(&self) -> Arc<dyn RuntimeActivity> {
        Arc::new(Unobserved)
    }

    fn next_periodic_due(&self) -> Option<SystemTime> {
        self.periodic_due.lock().ok().and_then(|due| *due)
    }

    fn undelivered_changes(&self) -> usize {
        self.backlog.load(Ordering::Relaxed)
    }
}

/// A factory recording which database each runtime was built against.
struct FakeFactory {
    /// Shared shutdown counter handed to every runtime it builds.
    shutdowns: Arc<AtomicUsize>,
    /// How many runtimes it has built.
    built: AtomicUsize,
    /// The periodic due cell of each tenant, shared with every runtime built
    /// for it.
    periodic_due: StdMutex<Vec<(TenantRef, PeriodicDueCell)>>,
    /// The backlog cell of each tenant, shared with every runtime built for it.
    backlogs: StdMutex<Vec<(TenantRef, BacklogCell)>>,
}

impl FakeFactory {
    /// A factory whose runtimes count their shutdowns on `shutdowns`.
    fn new(shutdowns: Arc<AtomicUsize>) -> FakeFactory {
        FakeFactory {
            shutdowns,
            built: AtomicUsize::new(0),
            periodic_due: StdMutex::new(Vec::new()),
            backlogs: StdMutex::new(Vec::new()),
        }
    }

    /// The backlog cell of `tenant`, created on first use.
    fn backlog_cell(&self, tenant: &TenantRef) -> BacklogCell {
        let mut cells = self.backlogs.lock().expect("the backlog cells are readable");
        if let Some((_, cell)) = cells.iter().find(|(held, _)| held == tenant) {
            return Arc::clone(cell);
        }
        let cell = BacklogCell::default();
        cells.push((tenant.clone(), Arc::clone(&cell)));
        cell
    }

    /// Have `tenant`'s runtime report `held` changes it has yet to act on.
    fn hold_changes(&self, tenant: &TenantRef, held: usize) {
        self.backlog_cell(tenant).store(held, Ordering::Relaxed);
    }

    /// The periodic due cell of `tenant`, created on first use.
    fn periodic_due_cell(&self, tenant: &TenantRef) -> PeriodicDueCell {
        let mut cells = self.periodic_due.lock().expect("the periodic due cells are readable");
        if let Some((_, cell)) = cells.iter().find(|(held, _)| held == tenant) {
            return Arc::clone(cell);
        }
        let cell = PeriodicDueCell::default();
        cells.push((tenant.clone(), Arc::clone(&cell)));
        cell
    }

    /// Have `tenant`'s runtime report its next periodic notification due at
    /// `due`, as its periodic sweep does after a pass.
    fn schedule_periodic(&self, tenant: &TenantRef, due: SystemTime) {
        *self.periodic_due_cell(tenant).lock().expect("the periodic due cell is writable") = Some(due);
    }
}

#[async_trait]
impl TenantStateFactory for FakeFactory {
    type State = String;
    type Runtime = FakeRuntime;

    async fn build(&self, tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
        self.built.fetch_add(1, Ordering::Relaxed);
        Ok((
            Arc::new(database.as_str().to_owned()),
            FakeRuntime {
                shutdowns: Arc::clone(&self.shutdowns),
                periodic_due: self.periodic_due_cell(tenant),
                backlog: self.backlog_cell(tenant),
            },
        ))
    }
}

/// A manager, its registry, its factory and the shutdown counter every
/// runtime it builds increments.
struct Harness {
    /// The manager under test.
    manager: Arc<TenantRuntimeManager<FakeRegistry, FakeFactory>>,
    /// The registry it resolves against.
    registry: Arc<FakeRegistry>,
    /// The factory it builds through.
    factory: Arc<FakeFactory>,
    /// How many runtimes have been released.
    shutdowns: Arc<AtomicUsize>,
}

fn harness(registered: &[&str], enabled: bool, max_cached: usize) -> Harness {
    let registry = Arc::new(FakeRegistry::new(registered));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(FakeFactory::new(Arc::clone(&shutdowns)));
    let manager = TenantRuntimeManager::new(
        config(enabled, max_cached),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );
    Harness {
        manager,
        registry,
        factory,
        shutdowns,
    }
}

#[tokio::test]
async fn the_unnamed_default_resolves_against_the_database_the_registry_names() {
    let Harness { manager, .. } = harness(&[], true, 64);
    let leased = manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("the default tenant resolves");
    pretty_assertions::assert_eq!(leased.state.as_str(), "andromeda");
}

#[tokio::test]
async fn a_second_request_for_one_tenant_reuses_the_runtime_the_first_built() {
    let Harness { manager, factory, .. } = harness(&["acme"], true, 64);
    let tenant = TenantRef::Named(named("acme"));

    let first = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");
    let second = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

    assert!(Arc::ptr_eq(&first.state, &second.state));
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1);
}

/// A factory whose first build meets an unreachable store, as a migration cut
/// short by a lost connection does, and whose later builds succeed.
struct FlakyFactory {
    /// How many builds were entered.
    built: AtomicUsize,
    /// Shared shutdown counter handed to every runtime it builds.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantStateFactory for FlakyFactory {
    type State = String;
    type Runtime = FakeRuntime;

    async fn build(&self, tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
        if self.built.fetch_add(1, Ordering::Relaxed) == 0 {
            return Err(TenancyError::RuntimeUnavailable {
                tenant: tenant.clone(), // clone: the error owns the tenant it names
                cause: RuntimeBuildFailure::Persistence(PersistenceFailure::Unavailable),
            });
        }
        Ok((Arc::new(database.as_str().to_owned()), FakeRuntime::unscheduled(&self.shutdowns)))
    }
}

#[tokio::test]
async fn a_tenant_whose_build_met_an_unreachable_store_builds_again_on_the_next_request() {
    let factory = Arc::new(FlakyFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::new(AtomicUsize::new(0)),
    });
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::new(FakeRegistry::new(&["acme"])),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );
    let tenant = TenantRef::Named(named("acme"));

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
    assert!(
        matches!(
            refused,
            Err(TenancyError::RuntimeUnavailable {
                cause: RuntimeBuildFailure::Persistence(PersistenceFailure::Unavailable),
                ..
            })
        ),
        "the first build is refused as an unreachable store"
    );
    let leased = manager
        .resolve_state(&tenant, TenantCreationPolicy::MustExist)
        .await
        .expect("the second build resolves");
    pretty_assertions::assert_eq!(leased.state.as_str(), "db_acme");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 2);
}

/// A factory whose every build blocks on a gate until the test opens it, so
/// concurrent builds overlap and single-flight is observable.
struct GatedFactory {
    /// How many builds were entered.
    built: AtomicUsize,
    /// Shared shutdown counter handed to every runtime it builds.
    shutdowns: Arc<AtomicUsize>,
    /// The gate every build waits on before completing.
    gate: Arc<Notify>,
}

#[async_trait]
impl TenantStateFactory for GatedFactory {
    type State = String;
    type Runtime = FakeRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
        self.built.fetch_add(1, Ordering::Relaxed);
        self.gate.notified().await;
        Ok((Arc::new(database.as_str().to_owned()), FakeRuntime::unscheduled(&self.shutdowns)))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_requests_for_one_tenant_trigger_a_single_build() {
    // Without single-flight, N concurrent first requests each open a whole
    // runtime and N-1 are thrown away. A gated factory holds every build
    // open so the requests overlap; single-flight lets exactly one through.
    let registry = Arc::new(FakeRegistry::new(&["acme"]));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::clone(&shutdowns),
        gate: Arc::clone(&gate),
    });
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );
    let tenant = TenantRef::Named(named("acme"));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let manager = Arc::clone(&manager);
        let tenant = tenant.clone(); // clone: each task owns its reference
        handles.push(tokio::spawn(async move {
            manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves")
        }));
    }

    // Let every task reach the build or the single-flight lock.
    sleep(Duration::from_millis(200)).await;
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "single-flight must build exactly once");

    gate.notify_waiters();
    let mut states = Vec::new();
    for handle in handles {
        states.push(handle.await.expect("task joins"));
    }
    for leased in &states {
        assert!(Arc::ptr_eq(&leased.state, &states[0].state), "every request resolves to the one built runtime");
    }
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1);
    // No runtime was built only to be discarded.
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn eviction_spares_a_runtime_used_since_it_was_chosen() {
    // Eviction snapshots last_used, then removes. A request that touches the
    // runtime in that window bumps last_used, and the conditional removal
    // must spare it, otherwise a request holding its Arc is handed a
    // torn-down runtime.
    let Harness { manager, shutdowns, .. } = harness(&["acme"], true, 64);
    let tenant = TenantRef::Named(named("acme"));
    let leased = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

    // A snapshot that no longer matches the runtime's last_used stands in for
    // a request that touched it after the sweep chose it: it must not evict.
    let evicted = manager.evict_all(vec![(tenant.clone(), Instant::now())], EvictionReason::Idle).await;
    pretty_assertions::assert_eq!(evicted, 0, "a runtime used since the snapshot must be spared");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    assert!(manager.runtimes.contains_key(&tenant), "the runtime is still held open");

    // Drop the lease: with nothing holding the runtime and the matching
    // snapshot (nothing touched it since), the idle sweep evicts it.
    drop(leased);
    let candidates = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(
        manager.evict_all(candidates, EvictionReason::Idle).await,
        1,
        "an untouched, unleased idle runtime is evicted"
    );
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_leased_runtime_survives_idle_and_excess_eviction() {
    // A request holding a lease pins its runtime against every sweep. With a
    // cap of one beside the unnamed default, acme is both idle (zero TTL) and
    // over the cap, yet neither sweep may take it while its lease is held.
    let Harness { manager, shutdowns, .. } = harness(&[], true, 1);
    let _default = manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("the default resolves");
    let acme = TenantRef::Named(named("acme"));
    let leased = manager.resolve_state(&acme, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");

    // Idle sweep at zero TTL: acme is idle but leased, so it is spared.
    let idle = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, 0, "a leased idle runtime is spared");
    // Excess trim at cap one: acme is over the cap but leased, so it is spared.
    pretty_assertions::assert_eq!(manager.trim_excess_runtimes(None).await, 0, "a leased excess runtime is spared");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    assert!(manager.runtimes.contains_key(&acme), "the leased runtime is still held open");

    // Once the lease drops, the idle sweep may take it.
    drop(leased);
    let idle = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, 1, "an unleased idle runtime is evicted");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn excess_trim_skips_a_leased_lru_and_takes_the_next() {
    // The excess trim drops the least-recently-used named runtimes. When the
    // LRU is leased it must be skipped and the next-oldest unleased runtime
    // taken instead, so the cap is honoured without tearing down an in-use
    // runtime.
    let Harness { manager, shutdowns, .. } = harness(&["a", "b", "c"], true, 2);

    // `a` is resolved first, so it is the LRU, and its lease is held. The
    // sleeps space the resolves apart so their last-used stamps differ.
    let leased_a = manager
        .resolve_state(&TenantRef::Named(named("a")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    sleep(Duration::from_millis(3)).await;
    // `b` next: the cache now holds {a, b} at cap 2, so nothing is trimmed.
    manager
        .resolve_state(&TenantRef::Named(named("b")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    sleep(Duration::from_millis(3)).await;
    // `c` pushes the cache to three at cap 2. The insert-trim must skip the
    // leased LRU `a` and take the next-oldest unleased runtime `b`.
    manager
        .resolve_state(&TenantRef::Named(named("c")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");

    // The insert-trim detaches the victim's teardown, so wait for it to settle
    // before counting shutdowns; the removal itself is synchronous.
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1, "exactly one runtime is trimmed");
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("a"))), "the leased LRU is spared");
    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("b"))),
        "the next-oldest unleased runtime is taken"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("c"))), "the just-built runtime is kept");
    drop(leased_a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_cold_build_is_never_evicted_by_another_builds_insert_trim() {
    // Twelve distinct tenants cold-build at once against a cap of four. Each
    // build's insert-trim sees the others' just-inserted runtimes, but every
    // one is leased by its still-in-flight request, so none is torn down: the
    // cache overshoots the cap transiently and no request is handed a closed
    // pool. Once the leases drop, a trim brings the cache back to the cap.
    const TENANTS: usize = 12;
    const CAP: usize = 4;

    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let registry = Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>()));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::clone(&shutdowns),
        gate: Arc::clone(&gate),
    });
    // A build-admission bound above the tenant count so this test exercises the
    // lease, not admission: all twelve must reach the gate at once.
    let manager = TenantRuntimeManager::new(
        bounded_config(CAP, TENANTS, 8),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );

    let mut handles = Vec::new();
    for index in 0..TENANTS {
        let manager = Arc::clone(&manager);
        let tenant = TenantRef::Named(named(&format!("t{index}")));
        handles.push(tokio::spawn(async move {
            manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves")
        }));
    }

    // Let every build reach the gate, so all twelve are cold-building at once.
    sleep(Duration::from_millis(300)).await;
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), TENANTS, "all twelve build concurrently");

    gate.notify_waiters();
    let mut leases = Vec::new();
    for handle in handles {
        leases.push(handle.await.expect("task joins"));
    }

    // While every lease is held, no runtime was evicted despite the cap, and
    // every request holds the live cached runtime for its tenant.
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0, "no leased runtime is torn down under the insert-trim");
    pretty_assertions::assert_eq!(manager.runtimes.len(), TENANTS, "the cache overshoots the cap while leases are held");
    for (index, leased) in leases.iter().enumerate() {
        let tenant = TenantRef::Named(named(&format!("t{index}")));
        let cached = manager.runtimes.get(&tenant).expect("the runtime is cached");
        assert!(Arc::ptr_eq(&leased.state, &cached.state), "request holds the live cached runtime");
    }

    // Dropping the leases lets the next trim bring the cache back to the cap.
    drop(leases);
    let trimmed = manager.trim_excess_runtimes(None).await;
    pretty_assertions::assert_eq!(trimmed, TENANTS - CAP, "the trim releases the overshoot once leases drop");
    pretty_assertions::assert_eq!(manager.runtimes.len(), CAP, "the cache settles at the cap");
}

#[tokio::test]
async fn an_unknown_tenant_is_rejected_under_must_exist_and_created_on_demand() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("acme"));

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
    assert!(matches!(refused, Err(TenancyError::UnknownTenant { .. })));
    pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 0);

    let created = manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");
    pretty_assertions::assert_eq!(created.state.as_str(), "db_acme");
    pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_named_tenant_is_refused_when_multi_tenancy_is_disabled() {
    let Harness { manager, .. } = harness(&["acme"], false, 64);

    let refused = manager.resolve_state(&TenantRef::Named(named("acme")), TenantCreationPolicy::MustExist).await;
    assert!(matches!(refused, Err(TenancyError::MultiTenancyDisabled)));

    assert!(manager.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await.is_ok());
}

#[tokio::test]
async fn a_suspended_tenant_is_refused_on_admission_and_never_cold_loaded() {
    // A data request bearing a suspended tenant's header is answered as if the
    // tenant did not exist (clause 5.5.10): admission refuses it with the
    // unknown-tenant error even under create-on-demand, and never provisions or
    // caches a runtime for it.
    let Harness { manager, registry, .. } = harness(&["acme"], true, 64);
    registry.suspend("acme");
    let tenant = TenantRef::Named(named("acme"));

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await;
    assert!(matches!(refused, Err(TenancyError::UnknownTenant { .. })));
    pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 0, "a suspended tenant is never provisioned");
    assert!(!manager.runtimes.contains_key(&tenant), "a suspended tenant is never cached");
}

#[tokio::test]
async fn a_suspended_tenant_is_not_woken() {
    // The waker skips a suspended tenant whose probe would otherwise report work
    // due, and clears its schedule so it is not re-claimed until it is resumed.
    let registry = Arc::new(FakeRegistry::new(&["acme"]));
    registry.suspend("acme");
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(FakeFactory::new(Arc::clone(&shutdowns)));
    let schedule = Arc::new(FakeSchedule::inert());
    let schedule_concrete = Arc::clone(&schedule);
    let schedule_handle: Arc<dyn TenantWakeSchedule> = schedule_concrete;
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::due_at(SystemTime::now())),
        schedule_handle,
    );

    let outcome = manager.wake(&named("acme")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::NothingDue);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 0, "a suspended tenant is not built");
    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("acme"))),
        "a suspended tenant is not cached"
    );
    let settled = schedule.settled.lock().expect("settled lock").clone();
    assert!(
        settled.iter().all(|(_, wake_at)| wake_at.is_none()),
        "a suspended tenant's schedule is cleared, never re-armed, got {settled:?}"
    );
}

#[tokio::test]
async fn a_tenant_removed_from_the_registry_has_its_runtime_released() {
    let Harness {
        manager, registry, shutdowns, ..
    } = harness(&["acme", "globex"], true, 64);
    for name in ["acme", "globex"] {
        manager
            .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
    }

    registry.forget("acme");
    pretty_assertions::assert_eq!(manager.evict_orphan_runtimes().await, 1);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn the_unnamed_default_is_never_evicted_as_an_orphan() {
    let Harness { manager, shutdowns, .. } = harness(&[], true, 64);
    manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");

    pretty_assertions::assert_eq!(manager.evict_orphan_runtimes().await, 0);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_registry_that_cannot_be_read_evicts_nothing_rather_than_everything() {
    /// A registry whose reads always fail.
    struct UnreadableRegistry;

    #[async_trait]
    impl TenantDatabase for UnreadableRegistry {
        fn default_database(&self) -> TenantDatabaseName {
            TenantDatabaseName::from("andromeda".to_string())
        }

        async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
            Err(TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable))
        }

        async fn registered_tenant(&self, name: &TenantName) -> Result<Option<RegisteredTenant>, TenancyError> {
            Ok(Some(RegisteredTenant::Established(TenantRecord::new(
                name.clone(), // clone: the record owns its name
                TenantStatus::Active,
                TenantDatabaseName::from(format!("db_{}", name.as_str())),
                epoch(),
                epoch(),
            ))))
        }

        async fn list_tenant_records(&self, _start: &TenantPageStart, _limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
            Err(TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable))
        }

        async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
            Ok(TenantDatabaseName::from(format!("db_{}", name.as_str())))
        }

        async fn establish(&self, _name: &TenantName) -> Result<Establishment, TenancyError> {
            Err(TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable))
        }

        async fn provisional_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
            Err(TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable))
        }

        async fn retire_provisional(&self, _name: &TenantName, _idle_for: Duration) -> Result<Retirement, TenancyError> {
            Err(TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable))
        }
    }

    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(FakeFactory::new(Arc::clone(&shutdowns)));
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::new(UnreadableRegistry),
        factory,
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );
    manager
        .resolve_state(&TenantRef::Named(named("acme")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");

    pretty_assertions::assert_eq!(manager.evict_orphan_runtimes().await, 0);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn resolving_past_the_limit_trims_the_lru_named_tenant_on_insert() {
    // No janitor is spawned in these tests, so the synchronous insert-trim is
    // the only trimming in effect: resolving a third tenant past a cap of two
    // must release the least-recently-used one during that resolve, not wait
    // for a sweep that never runs. Each resolve drops its lease at the end of
    // the statement, so the earlier tenants are unleased when the trim runs.
    let Harness { manager, shutdowns, .. } = harness(&["a", "b", "c"], true, 2);
    for name in ["a", "b", "c"] {
        manager
            .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
        // Space the resolves apart so their millisecond last-used stamps differ
        // and `a` is unambiguously the least-recently-used entry to trim.
        sleep(Duration::from_millis(3)).await;
    }

    pretty_assertions::assert_eq!(manager.runtimes.len(), 2);
    // The removal is synchronous; the teardown is detached, so drain it before
    // counting shutdowns.
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("a"))),
        "the least-recently-used tenant is evicted"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("b"))));
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("c"))));
}

#[tokio::test]
async fn insert_trim_keeps_the_just_built_runtime_at_capacity_one() {
    // With a cap of one beside the unnamed default, the freshly built named
    // runtime is itself over the cap. Its own lease, and `protect`, must
    // spare it, so the resolve hands back a live runtime rather than one the
    // trim just tore down.
    let Harness { manager, shutdowns, .. } = harness(&[], true, 1);
    manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("the default resolves");
    let acme = TenantRef::Named(named("acme"));
    let built = manager.resolve_state(&acme, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");

    assert!(
        manager.runtimes.contains_key(&TenantRef::UnnamedDefault),
        "the unnamed default is never evicted"
    );
    assert!(manager.runtimes.contains_key(&acme), "the just-built runtime is protected from its own trim");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);

    // The returned state is the live cached one: a follow-up hit resolves to
    // the same Arc, proving the trim did not close the pool behind it. The hit
    // may create the tenant too, since nothing has been written to it yet.
    let hit = manager.resolve_state(&acme, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");
    assert!(Arc::ptr_eq(&built.state, &hit.state));
}

#[tokio::test]
async fn insert_trim_is_a_noop_within_the_limit() {
    // Resolving up to but not past the cap trims nothing: the boundary partner
    // to the over-limit case, guarding against over-eager eviction.
    let Harness { manager, shutdowns, .. } = harness(&["a", "b"], true, 2);
    for name in ["a", "b"] {
        manager
            .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
    }

    pretty_assertions::assert_eq!(manager.runtimes.len(), 2);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_cache_inside_its_limit_drops_nothing() {
    let Harness { manager, .. } = harness(&["a", "b"], true, 2);
    for name in ["a", "b"] {
        manager
            .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
    }
    let excess = manager.excess_runtimes();
    assert!(excess.is_empty(), "{excess:?}");
}

#[tokio::test]
async fn a_runtime_older_than_the_ttl_is_idle_and_the_default_never_is() {
    let Harness { manager, shutdowns, .. } = harness(&["acme"], true, 64);
    manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    manager
        .resolve_state(&TenantRef::Named(named("acme")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");

    // A zero TTL makes every runtime idle the instant it is inspected.
    let idle = manager.idle_runtimes(Duration::ZERO);
    let idle_tenants: Vec<TenantRef> = idle.iter().map(|(tenant, _)| tenant.clone()).collect();
    pretty_assertions::assert_eq!(idle_tenants, vec![TenantRef::Named(named("acme"))]);
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, 1);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_single_tenant_broker_starts_no_janitor() {
    let Harness { manager, .. } = harness(&[], false, 64);
    assert!(manager.spawn_janitor().is_none());
}

/// A factory that records how many builds run at once, so the admission bound
/// is observable.
struct BuildConcurrencyProbe {
    /// Builds currently in flight.
    active: AtomicUsize,
    /// The highest concurrent build count observed.
    peak: AtomicUsize,
    /// How long each build is held so concurrent builds overlap.
    hold: Duration,
    /// Shared shutdown counter handed to every runtime it builds.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantStateFactory for BuildConcurrencyProbe {
    type State = String;
    type Runtime = FakeRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
        let concurrent = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(concurrent, Ordering::SeqCst);
        sleep(self.hold).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok((Arc::new(database.as_str().to_owned()), FakeRuntime::unscheduled(&self.shutdowns)))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_builds_across_tenants_never_exceed_the_configured_bound() {
    // Twelve distinct tenants cold-build at once against a build-admission bound
    // of three. Each build holds long enough to overlap the others, so without
    // the semaphore all twelve would run at once; the bound must hold the peak
    // at three. The cache is large enough that no trim interferes.
    const TENANTS: usize = 12;
    const BOUND: usize = 3;

    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let registry = Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>()));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(BuildConcurrencyProbe {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        hold: Duration::from_millis(50),
        shutdowns: Arc::clone(&shutdowns),
    });
    let manager = TenantRuntimeManager::new(
        bounded_config(TENANTS, BOUND, 8),
        Arc::clone(&registry),
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );

    let mut handles = Vec::new();
    for index in 0..TENANTS {
        let manager = Arc::clone(&manager);
        let tenant = TenantRef::Named(named(&format!("t{index}")));
        handles.push(tokio::spawn(async move {
            drop(manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves"));
        }));
    }
    for handle in handles {
        handle.await.expect("task joins");
    }

    let peak = factory.peak.load(Ordering::SeqCst);
    assert!(peak <= BOUND, "no more than {BOUND} cold builds may run at once, but {peak} did");
    assert!(peak > 1, "the builds must overlap for the bound to mean anything, but the peak was {peak}");
}

/// A runtime whose shutdown records how many teardowns run at once, so the
/// eviction bound is observable.
struct ShutdownConcurrencyProbe {
    /// Teardowns currently in flight.
    active: Arc<AtomicUsize>,
    /// The highest concurrent teardown count observed.
    peak: Arc<AtomicUsize>,
    /// How long each teardown is held so concurrent teardowns overlap.
    hold: Duration,
}

#[async_trait]
impl TenantRuntime for ShutdownConcurrencyProbe {
    async fn shutdown(self) {
        let concurrent = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(concurrent, Ordering::SeqCst);
        sleep(self.hold).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
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
}

/// A factory that builds [`ShutdownConcurrencyProbe`] runtimes sharing one pair
/// of concurrency counters.
struct ShutdownProbeFactory {
    /// Teardowns currently in flight, shared by every runtime it builds.
    active: Arc<AtomicUsize>,
    /// The highest concurrent teardown count observed.
    peak: Arc<AtomicUsize>,
    /// How long each teardown is held.
    hold: Duration,
}

#[async_trait]
impl TenantStateFactory for ShutdownProbeFactory {
    type State = String;
    type Runtime = ShutdownConcurrencyProbe;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, ShutdownConcurrencyProbe), TenancyError> {
        Ok((
            Arc::new(database.as_str().to_owned()),
            ShutdownConcurrencyProbe {
                active: Arc::clone(&self.active),
                peak: Arc::clone(&self.peak),
                hold: self.hold,
            },
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn evictions_overlap_up_to_the_bound() {
    // Six idle, unleased runtimes are evicted at once against an eviction bound
    // of two. Their teardowns run concurrently, so the peak overlap must reach
    // the bound but never exceed it.
    const TENANTS: usize = 6;
    const BOUND: usize = 2;

    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let registry = Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>()));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(ShutdownProbeFactory {
        active: Arc::clone(&active),
        peak: Arc::clone(&peak),
        hold: Duration::from_millis(50),
    });
    let manager = TenantRuntimeManager::new(
        bounded_config(TENANTS, 8, BOUND),
        Arc::clone(&registry),
        factory,
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );

    for index in 0..TENANTS {
        manager
            .resolve_state(&TenantRef::Named(named(&format!("t{index}"))), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
    }

    let idle = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(idle.len(), TENANTS, "every runtime is idle at zero TTL");
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, TENANTS, "all six are evicted");

    let observed = peak.load(Ordering::SeqCst);
    assert!(observed <= BOUND, "no more than {BOUND} teardowns may overlap, but {observed} did");
    assert!(
        observed > 1,
        "the teardowns must overlap for the bound to mean anything, but the peak was {observed}"
    );
}

/// A runtime whose shutdown blocks on a gate until the test opens it, so a
/// detached teardown can be observed to run off the request path.
struct GatedShutdownRuntime {
    /// The gate the teardown waits on before it completes.
    gate: Arc<Notify>,
    /// Incremented once the teardown passes the gate.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantRuntime for GatedShutdownRuntime {
    async fn shutdown(self) {
        self.gate.notified().await;
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
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
}

/// A factory building [`GatedShutdownRuntime`]s that share one gate.
struct GatedShutdownFactory {
    /// The gate every teardown waits on.
    gate: Arc<Notify>,
    /// Shared shutdown counter.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantStateFactory for GatedShutdownFactory {
    type State = String;
    type Runtime = GatedShutdownRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, GatedShutdownRuntime), TenancyError> {
        Ok((
            Arc::new(database.as_str().to_owned()),
            GatedShutdownRuntime {
                gate: Arc::clone(&self.gate),
                shutdowns: Arc::clone(&self.shutdowns),
            },
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn insert_trim_does_not_await_victim_shutdown() {
    // With a cap of one, resolving `b` past `a` trims `a`. Its teardown blocks
    // on the gate, yet the resolve must return: the removal is synchronous, the
    // teardown detached. Opening the gate then lets the detached teardown run.
    let registry = Arc::new(FakeRegistry::new(&["a", "b"]));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedShutdownFactory {
        gate: Arc::clone(&gate),
        shutdowns: Arc::clone(&shutdowns),
    });
    let manager = TenantRuntimeManager::new(
        config(true, 1),
        Arc::clone(&registry),
        factory,
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );

    // Resolve `a`; its lease drops at the statement's end, so `a` is unleased.
    manager
        .resolve_state(&TenantRef::Named(named("a")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    // Space the resolves so `a` is unambiguously the least-recently-used entry.
    sleep(Duration::from_millis(3)).await;

    // Resolving `b` triggers the insert-trim of `a`. If the trim awaited `a`'s
    // gated teardown this would hang, so a timeout turns that into a failure.
    let resolved_b = timeout(
        Duration::from_secs(2),
        manager.resolve_state(&TenantRef::Named(named("b")), TenantCreationPolicy::MustExist),
    )
    .await
    .expect("the cold insert returns without awaiting the blocked teardown")
    .expect("resolves");
    drop(resolved_b);

    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("a"))),
        "the victim is removed synchronously"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("b"))), "the just-built runtime is kept");
    pretty_assertions::assert_eq!(
        shutdowns.load(Ordering::SeqCst),
        0,
        "the teardown is detached, so it has not run while the gate is shut"
    );

    // Open the gate and drain the eviction set: the detached teardown completes.
    gate.notify_one();
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::SeqCst), 1, "the detached teardown eventually runs");
}

/// A manager over the given probe and schedule, plus its factory and shutdown
/// counter, for the dormant-wake tests.
fn wake_manager(
    registered: &[&str],
    max_cached: usize,
    probe: Arc<FakeProbe>,
    schedule: Arc<FakeSchedule>,
) -> (Arc<TenantRuntimeManager<FakeRegistry, FakeFactory>>, Arc<FakeFactory>, Arc<AtomicUsize>) {
    let registry = Arc::new(FakeRegistry::new(registered));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(FakeFactory::new(Arc::clone(&shutdowns)));
    let manager = TenantRuntimeManager::new(config(true, max_cached), registry, Arc::clone(&factory), probe, schedule);
    (manager, factory, shutdowns)
}

#[tokio::test]
async fn an_idle_tenant_with_work_due_inside_the_ttl_is_retained() {
    // A dormant-eligible tenant whose probe reports work already due is kept
    // resident, not evicted: background work counts as use.
    let probe = Arc::new(FakeProbe::due_at(SystemTime::UNIX_EPOCH));
    let (manager, _factory, shutdowns) = wake_manager(&["acme"], 64, Arc::clone(&probe), Arc::new(FakeSchedule::inert()));
    let acme = TenantRef::Named(named("acme"));
    // Resolve then drop the lease so acme is unleased and idle at zero TTL.
    drop(manager.resolve_state(&acme, TenantCreationPolicy::MustExist).await.expect("resolves"));

    let evicted = manager.sweep_idle_dormant(Duration::ZERO).await;
    pretty_assertions::assert_eq!(probe.asked(), vec![TenantExistence::Established]);
    pretty_assertions::assert_eq!(evicted, 0, "a tenant with imminent work is retained, not evicted");
    assert!(manager.runtimes.contains_key(&acme), "the tenant stays resident to do its work");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn waking_a_cached_tenant_settles_the_row_without_touching_last_used() {
    let schedule = Arc::new(FakeSchedule::inert());
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::new(FakeProbe::none()), Arc::clone(&schedule));
    let acme = TenantRef::Named(named("acme"));
    drop(manager.resolve_state(&acme, TenantCreationPolicy::MustExist).await.expect("resolves"));
    let before = manager.runtimes.get(&acme).expect("cached").last_used_instant();
    sleep(Duration::from_millis(5)).await;

    let outcome = manager.wake(&named("acme")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::AlreadyCached);
    let after = manager.runtimes.get(&acme).expect("still cached").last_used_instant();
    pretty_assertions::assert_eq!(before, after, "waking a cached tenant does not touch its last use");
    pretty_assertions::assert_eq!(schedule.settle_count(), 1, "the schedule row is settled");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "a cached tenant is not rebuilt");
}

#[tokio::test]
async fn a_claimed_tenant_whose_work_falls_due_as_the_probe_answers_is_loaded() {
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::new(FakeProbe::due_as_answered()), Arc::new(FakeSchedule::inert()));

    let outcome = manager.wake(&named("acme")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::Loaded);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "work due now is run, not re-armed");
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("acme"))), "the woken tenant is cached");
}

#[tokio::test]
async fn a_claimed_tenant_with_nothing_due_is_not_loaded() {
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::new(FakeProbe::none()), Arc::new(FakeSchedule::inert()));

    let outcome = manager.wake(&named("acme")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::NothingDue);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 0, "nothing due, so nothing is loaded");
    assert!(!manager.runtimes.contains_key(&TenantRef::Named(named("acme"))), "the tenant is not cached");
}

#[tokio::test]
async fn a_full_cache_of_unleased_runtimes_yields_its_lru_to_a_due_wake() {
    // A tenant whose work is due now must run rather than defer behind runtimes
    // that may have no work at all, so at capacity it displaces the
    // least-recently-used unleased runtime and the connection budget stays flat.
    let (manager, factory, _shutdowns) = wake_manager(
        &["lru", "fresh", "woken"],
        2,
        Arc::new(FakeProbe::due_at(SystemTime::now())),
        Arc::new(FakeSchedule::inert()),
    );
    let lru = TenantRef::Named(named("lru"));
    drop(manager.resolve_state(&lru, TenantCreationPolicy::MustExist).await.expect("resolves"));
    // Space the two touches so `lru` sorts strictly older than `fresh` and is the
    // unambiguous displacement victim.
    sleep(Duration::from_millis(10)).await;
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("fresh")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    pretty_assertions::assert_eq!(manager.runtimes.len(), 2, "the cache is at capacity");

    let outcome = manager.wake(&named("woken")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::Loaded);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 3, "the due wake builds its runtime");
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("woken"))), "the woken tenant is cached");
    assert!(!manager.runtimes.contains_key(&lru), "the least-recently-used runtime was displaced");
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("fresh"))), "the fresher runtime is kept");
    pretty_assertions::assert_eq!(manager.runtimes.len(), 2, "the connection budget stays flat: one in, one out");
}

#[tokio::test]
async fn a_due_wake_defers_only_when_every_runtime_is_leased() {
    // Genuine backpressure: the cache is full and every runtime is held by an
    // in-flight request, so no slot can be freed and the wake defers rather than
    // exceeding the connection budget.
    let (manager, factory, _shutdowns) = wake_manager(
        &["held", "woken"],
        1,
        Arc::new(FakeProbe::due_at(SystemTime::now())),
        Arc::new(FakeSchedule::inert()),
    );
    // Keep the lease alive: an in-flight request still holds `held`.
    let _lease = manager
        .resolve_state(&TenantRef::Named(named("held")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    pretty_assertions::assert_eq!(manager.runtimes.len(), 1, "the cache is at capacity with a leased runtime");

    let outcome = manager.wake(&named("woken")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::Deferred);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "the deferred wake builds nothing");
    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("woken"))),
        "the deferred tenant is not cached"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("held"))), "the leased runtime is spared");
}

/// A manager like [`wake_manager`] whose woken runtimes stay resident for
/// `woken_lifetime`.
fn wake_manager_with_lifetime(
    registered: &[&str],
    max_cached: usize,
    woken_lifetime: HumanDuration,
) -> (Arc<TenantRuntimeManager<FakeRegistry, FakeFactory>>, Arc<AtomicUsize>) {
    let registry = Arc::new(FakeRegistry::new(registered));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(FakeFactory::new(Arc::clone(&shutdowns)));
    let base = config(true, max_cached);
    let config = TenancyConfig {
        residency: ResidencyConfig {
            woken_lifetime,
            ..base.residency
        },
        ..base
    };
    let manager = TenantRuntimeManager::new(
        config,
        registry,
        factory,
        Arc::new(FakeProbe::due_at(SystemTime::now())),
        Arc::new(FakeSchedule::inert()),
    );
    (manager, shutdowns)
}

/// A woken runtime is backdated, so a cold build making room would pick it
/// first; within its wake residency it is spared, so the work it was woken
/// for runs instead of the tenant being unloaded and woken again for it.
#[tokio::test]
async fn a_cold_build_does_not_unload_a_woken_runtime_within_its_wake_residency() {
    let (manager, _shutdowns) = wake_manager_with_lifetime(&["fresh", "woken", "next"], 2, HumanDuration::from_secs(60));
    let fresh = TenantRef::Named(named("fresh"));
    drop(manager.resolve_state(&fresh, TenantCreationPolicy::MustExist).await.expect("resolves"));
    pretty_assertions::assert_eq!(manager.wake(&named("woken")).await, WakeOutcome::Loaded);

    drop(
        manager
            .resolve_state(&TenantRef::Named(named("next")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );

    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("woken"))), "the woken runtime is spared");
    assert!(!manager.runtimes.contains_key(&fresh), "the request-touched runtime made room instead");
    pretty_assertions::assert_eq!(manager.runtimes.len(), 2);
}

/// Once its wake residency has passed, a woken runtime is again the first
/// runtime unloaded to make room.
#[tokio::test]
async fn a_cold_build_unloads_a_woken_runtime_once_its_wake_residency_has_passed() {
    let (manager, _shutdowns) = wake_manager_with_lifetime(&["fresh", "woken", "next"], 2, HumanDuration::from_millis(20));
    let fresh = TenantRef::Named(named("fresh"));
    drop(manager.resolve_state(&fresh, TenantCreationPolicy::MustExist).await.expect("resolves"));
    pretty_assertions::assert_eq!(manager.wake(&named("woken")).await, WakeOutcome::Loaded);
    sleep(Duration::from_millis(40)).await;

    drop(
        manager
            .resolve_state(&TenantRef::Named(named("next")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );

    assert!(!manager.runtimes.contains_key(&TenantRef::Named(named("woken"))), "the woken runtime made room");
    assert!(manager.runtimes.contains_key(&fresh), "the request-touched runtime is kept");
}

/// A wake making room does not take another woken runtime still within its
/// residency: with nothing else to unload, it defers.
#[tokio::test]
async fn a_due_wake_does_not_displace_a_woken_runtime_within_its_wake_residency() {
    let (manager, _shutdowns) = wake_manager_with_lifetime(&["first", "second"], 1, HumanDuration::from_secs(60));
    pretty_assertions::assert_eq!(manager.wake(&named("first")).await, WakeOutcome::Loaded);

    pretty_assertions::assert_eq!(manager.wake(&named("second")).await, WakeOutcome::Deferred);
    assert!(
        manager.runtimes.contains_key(&TenantRef::Named(named("first"))),
        "the first woken runtime is spared"
    );
}

/// The deferred wake is retried as the woken lifetime that blocked it ends,
/// well before the default `min_wake_interval` of 30 s.
#[tokio::test]
async fn a_wake_deferred_by_a_woken_runtime_is_retried_when_its_woken_lifetime_ends() {
    let base = config(true, 1);
    let tenancy = TenancyConfig {
        residency: ResidencyConfig {
            woken_lifetime: HumanDuration::from_secs(2),
            ..base.residency
        },
        ..base
    };
    let schedule = Arc::new(FakeSchedule::inert());
    let manager = TenantRuntimeManager::new(
        tenancy,
        Arc::new(FakeRegistry::new(&["first", "second"])),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::new(FakeProbe::due_at(SystemTime::now())),
        Arc::clone(&schedule) as Arc<dyn TenantWakeSchedule>,
    );
    pretty_assertions::assert_eq!(manager.wake(&named("first")).await, WakeOutcome::Loaded);
    let deferred_at = SystemTime::now();

    pretty_assertions::assert_eq!(manager.wake(&named("second")).await, WakeOutcome::Deferred);

    let settled = schedule.settled.lock().expect("settled lock").clone();
    let retry = settled
        .iter()
        .find_map(|(tenant, wake_at)| (tenant == "second").then_some(*wake_at))
        .flatten()
        .expect("the deferred wake is re-armed");
    let wait = retry.duration_since(deferred_at).expect("the retry is ahead");
    assert!(
        wait > Duration::from_secs(1) && wait <= Duration::from_secs(2),
        "retried as the first runtime's woken lifetime ends, got {wait:?}"
    );
}

#[tokio::test]
async fn a_woken_runtime_is_the_first_lru_victim() {
    // A woken runtime is inserted backdated, so it sorts older than any
    // request-touched runtime and is the first to be trimmed.
    let (manager, factory, _shutdowns) = wake_manager(
        &["fresh", "woken"],
        64,
        Arc::new(FakeProbe::due_at(SystemTime::now())),
        Arc::new(FakeSchedule::inert()),
    );
    let fresh = TenantRef::Named(named("fresh"));
    drop(manager.resolve_state(&fresh, TenantCreationPolicy::MustExist).await.expect("resolves"));
    // Space and re-touch so the request-touched runtime's last use is a clear
    // positive offset from the process epoch, above the backdated woken stamp.
    sleep(Duration::from_millis(10)).await;
    drop(manager.resolve_state(&fresh, TenantCreationPolicy::MustExist).await.expect("resolves"));

    let outcome = manager.wake(&named("woken")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::Loaded);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 2, "the woken tenant is built");

    let woken_used = manager
        .runtimes
        .get(&TenantRef::Named(named("woken")))
        .expect("woken cached")
        .last_used_instant();
    let fresh_used = manager.runtimes.get(&fresh).expect("fresh cached").last_used_instant();
    assert!(
        woken_used < fresh_used,
        "the woken runtime sorts older than a request-touched one, so it is the first victim"
    );
}

#[tokio::test]
async fn a_claimed_tenant_with_due_work_is_loaded() {
    // The positive counterpart to `a_claimed_tenant_with_nothing_due_is_not_loaded`:
    // a non-cached tenant whose probe reports work due now is loaded by the wake.
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::new(FakeProbe::due_at(SystemTime::now())), Arc::new(FakeSchedule::inert()));

    let outcome = manager.wake(&named("acme")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::Loaded);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "a claimed tenant with due work is loaded");
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("acme"))), "the woken tenant is cached");
}

#[tokio::test]
async fn a_database_gone_probe_clears_the_schedule_rather_than_retrying() {
    // A tenant whose database no longer exists (deprovisioned under a stale
    // schedule row) must have its schedule cleared, not re-armed with a retry,
    // otherwise it churns the wake pass every dormancy interval and starves the
    // tenants that really have work.
    let schedule = Arc::new(FakeSchedule::inert());
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::new(FakeProbe::database_gone()), Arc::clone(&schedule));

    let outcome = manager.wake(&named("acme")).await;
    pretty_assertions::assert_eq!(outcome, WakeOutcome::NothingDue);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 0, "a gone tenant is not loaded");
    let settled = schedule.settled.lock().expect("settled lock").clone();
    pretty_assertions::assert_eq!(settled.len(), 1, "the gone tenant's schedule is settled once");
    assert!(
        settled.iter().all(|(_, wake_at)| wake_at.is_none()),
        "a gone tenant's schedule is cleared, never re-armed with a retry instant, got {settled:?}"
    );
}

/// Provisioning commits the provisional row before it creates and migrates the
/// database, so a wake can probe a database that holds no schema yet.
#[tokio::test]
async fn an_unmigrated_provisional_tenant_is_not_loaded_and_its_schedule_is_cleared() {
    let schedule = Arc::new(FakeSchedule::inert());
    let probe = Arc::new(FakeProbe::unmigrated());
    let (manager, factory, _shutdowns) = wake_manager(&[], 64, Arc::clone(&probe), Arc::clone(&schedule));
    manager.database.register_provisional("acme");

    pretty_assertions::assert_eq!(manager.wake(&named("acme")).await, WakeOutcome::NothingDue);
    pretty_assertions::assert_eq!(probe.asked(), vec![TenantExistence::Provisional]);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 0, "a tenant still being provisioned is not loaded");
    let settled = schedule.settled.lock().expect("settled lock").clone();
    pretty_assertions::assert_eq!(settled.len(), 1);
    assert!(
        settled.iter().all(|(_, wake_at)| wake_at.is_none()),
        "the schedule is cleared, never re-armed with a retry instant, got {settled:?}"
    );
}

#[tokio::test]
async fn a_dormant_tenant_whose_work_is_not_due_yet_is_rescheduled_for_its_due_instant_however_soon() {
    // Well within the default `min_wake_interval` of 30 s, which holds off only
    // the wake an eviction records.
    let due = SystemTime::now() + Duration::from_secs(2);
    let schedule = Arc::new(FakeSchedule::inert());
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::new(FakeProbe::due_at(due)), Arc::clone(&schedule));

    pretty_assertions::assert_eq!(manager.wake(&named("acme")).await, WakeOutcome::NothingDue);

    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 0);
    let settled = schedule.settled.lock().expect("settled lock").clone();
    pretty_assertions::assert_eq!(settled, vec![("acme".to_string(), Some(due))]);
}

#[tokio::test]
async fn an_unmigrated_established_tenant_is_loaded_so_its_build_migrates_it() {
    let probe = Arc::new(FakeProbe::unmigrated());
    let (manager, factory, _shutdowns) = wake_manager(&["acme"], 64, Arc::clone(&probe), Arc::new(FakeSchedule::inert()));

    pretty_assertions::assert_eq!(manager.wake(&named("acme")).await, WakeOutcome::Loaded);
    pretty_assertions::assert_eq!(probe.asked(), vec![TenantExistence::Established]);
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1);
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("acme"))));
}

#[tokio::test]
async fn an_excess_evicted_tenant_with_due_work_is_scheduled_for_wake() {
    // Cache pressure evicts through the excess path, not the idle sweep, so an
    // excess victim with due work must still be scheduled for a wake, otherwise
    // a dormant tenant evicted under pressure is only found by the slow
    // reconciliation sweep. A victim with nothing due records a null wake and is
    // never claimed, so an obligation-free workload adds no wake load.
    let due = SystemTime::now() + Duration::from_secs(5);
    let schedule = Arc::new(FakeSchedule::inert());
    let (manager, _factory, _shutdowns) = wake_manager(&["keep", "victim"], 1, Arc::new(FakeProbe::due_at(due)), Arc::clone(&schedule));
    // Resolve the victim first so it is the older, unleased entry; the second cold
    // insert trims it back to the cache bound of one.
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("victim")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    sleep(Duration::from_millis(5)).await;
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("keep")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    manager.trim_excess_runtimes(None).await;
    manager.await_evictions().await;
    let recorded = schedule.recorded();
    assert!(
        recorded.iter().any(|(tenant, wake_at)| tenant == "victim" && wake_at.is_some()),
        "an excess victim with due work is scheduled for a wake, recorded {recorded:?}"
    );

    let nothing = Arc::new(FakeSchedule::inert());
    let (manager, _factory, _shutdowns) = wake_manager(&["keep2", "victim2"], 1, Arc::new(FakeProbe::none()), Arc::clone(&nothing));
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("victim2")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    sleep(Duration::from_millis(5)).await;
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("keep2")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    manager.trim_excess_runtimes(None).await;
    manager.await_evictions().await;
    let recorded = nothing.recorded();
    assert!(
        recorded.iter().any(|(tenant, wake_at)| tenant == "victim2" && wake_at.is_none()),
        "a nothing-due excess victim records a null wake and is not claimed, recorded {recorded:?}"
    );
}

/// A runtime that marks a shared flag committed as it shuts down, modelling an
/// in-flight delivery that becomes visible in the database only once the runtime
/// has drained.
struct CommitOnShutdownRuntime {
    /// Set true once this runtime has finished shutting down.
    committed: Arc<AtomicBool>,
    /// Incremented once the teardown completes.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantRuntime for CommitOnShutdownRuntime {
    async fn shutdown(self) {
        // A brief drain, so a probe that ran before this shutdown would observe
        // the obligation as not yet committed.
        sleep(Duration::from_millis(20)).await;
        self.committed.store(true, Ordering::SeqCst);
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
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
}

/// A factory building [`CommitOnShutdownRuntime`]s that share one commit flag.
struct CommitOnShutdownFactory {
    /// The flag every runtime it builds marks committed on shutdown.
    committed: Arc<AtomicBool>,
    /// The shared shutdown counter every runtime it builds increments.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantStateFactory for CommitOnShutdownFactory {
    type State = String;
    type Runtime = CommitOnShutdownRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, CommitOnShutdownRuntime), TenancyError> {
        Ok((
            Arc::new(database.as_str().to_owned()),
            CommitOnShutdownRuntime {
                committed: Arc::clone(&self.committed),
                shutdowns: Arc::clone(&self.shutdowns),
            },
        ))
    }
}

/// A probe that reports work due only once a shared flag is committed, modelling
/// an obligation that lands in the database as the runtime drains: a probe run
/// before the shutdown sees nothing, one run after sees the due work.
struct CommitGatedProbe {
    /// The flag a [`CommitOnShutdownRuntime`] sets as it shuts down.
    committed: Arc<AtomicBool>,
    /// The instant the work is due once it is visible.
    due: SystemTime,
}

#[async_trait]
impl PendingWorkProbe for CommitGatedProbe {
    async fn earliest_pending_work(&self, _database: &TenantDatabaseName, _existence: TenantExistence) -> Result<PendingWork, PendingWorkError> {
        if self.committed.load(Ordering::SeqCst) {
            Ok(PendingWork::DueAt(self.due))
        } else {
            Ok(PendingWork::None)
        }
    }
}

#[tokio::test]
async fn the_eviction_probe_runs_after_shutdown_so_a_committed_obligation_is_seen() {
    // An excess-evicted tenant's obligation may commit only as its runtime drains
    // (a durable delivery mid-flight). A wake probe that ran before the shutdown
    // would see nothing and record a null wake, stranding the tenant until the
    // slow reconciliation sweep. Modelled with a probe that reports due work only
    // once the runtime's shutdown has set the shared commit flag: the recorded
    // wake must be due, never null, proving the probe read after the shutdown.
    let committed = Arc::new(AtomicBool::new(false));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let due = SystemTime::now();
    let registry = Arc::new(FakeRegistry::new(&["keep", "victim"]));
    let factory = Arc::new(CommitOnShutdownFactory {
        committed: Arc::clone(&committed),
        shutdowns: Arc::clone(&shutdowns),
    });
    let schedule = Arc::new(FakeSchedule::inert());
    let probe = Arc::new(CommitGatedProbe {
        committed: Arc::clone(&committed),
        due,
    });
    let schedule_concrete = Arc::clone(&schedule);
    let schedule_handle: Arc<dyn TenantWakeSchedule> = schedule_concrete;
    let manager = TenantRuntimeManager::new(config(true, 1), Arc::clone(&registry), factory, probe, schedule_handle);

    // Resolve the victim first so it is the older, unleased entry; the cold insert
    // of `keep` trims it back to the cache bound of one.
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("victim")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    sleep(Duration::from_millis(5)).await;
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("keep")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves"),
    );
    manager.trim_excess_runtimes(None).await;
    manager.await_evictions().await;

    assert!(shutdowns.load(Ordering::SeqCst) >= 1, "the excess victim is shut down");
    let recorded = schedule.recorded();
    assert!(
        recorded.iter().any(|(tenant, wake_at)| tenant == "victim" && wake_at.is_some()),
        "the wake probe runs after shutdown, so a just-committed obligation is scheduled with a due wake, recorded {recorded:?}"
    );
}

/// A runtime whose teardown waits until the test closes a shared latch, so an
/// evicted runtime stays alive for exactly as long as the test chooses.
struct LatchedShutdownRuntime {
    /// Closed by the test to let every teardown complete.
    latch: Arc<Semaphore>,
    /// How many runtimes are alive: built and not yet torn down.
    alive: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantRuntime for LatchedShutdownRuntime {
    async fn shutdown(self) {
        // The latch never carries a permit; closing it is what releases the
        // waiting teardowns.
        let _closed = self.latch.acquire().await;
        self.alive.fetch_sub(1, Ordering::SeqCst);
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
}

/// A factory building [`LatchedShutdownRuntime`]s that share one latch and one
/// count of live runtimes.
struct LatchedShutdownFactory {
    /// The latch every teardown waits on.
    latch: Arc<Semaphore>,
    /// How many runtimes are alive.
    alive: Arc<AtomicUsize>,
    /// How many runtimes it has built.
    built: AtomicUsize,
}

#[async_trait]
impl TenantStateFactory for LatchedShutdownFactory {
    type State = String;
    type Runtime = LatchedShutdownRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, LatchedShutdownRuntime), TenancyError> {
        self.built.fetch_add(1, Ordering::SeqCst);
        self.alive.fetch_add(1, Ordering::SeqCst);
        Ok((
            Arc::new(database.as_str().to_owned()),
            LatchedShutdownRuntime {
                latch: Arc::clone(&self.latch),
                alive: Arc::clone(&self.alive),
            },
        ))
    }
}

/// A config whose admission deadline is short enough for a test to reach it.
fn deadline_config(max_cached: usize, max_concurrent_builds: usize, eviction_concurrency: usize) -> TenancyConfig {
    let bounded = bounded_config(max_cached, max_concurrent_builds, eviction_concurrency);
    TenancyConfig {
        build: TenantBuildConfig {
            admission_timeout: HumanDuration::from_millis(300),
            ..bounded.build
        },
        ..bounded
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_runtimes_never_exceed_the_cache_plus_the_teardown_slots_while_teardowns_stall() {
    // Evicted runtimes whose teardown stalls (a pool close queued behind a
    // saturated pooler) must not pile up alive behind the teardown bound. With a
    // cap of two and two teardown slots, the third and fourth cold builds each
    // take a slot to evict the LRU; the fifth finds no slot free within the
    // admission deadline and is refused as saturated instead of adding a fifth
    // live runtime. Once the teardowns complete, the same build is admitted.
    const CAP: usize = 2;
    const SLOTS: usize = 2;

    let registry = Arc::new(FakeRegistry::new(&["t0", "t1", "t2", "t3", "t4"]));
    let latch = Arc::new(Semaphore::new(0));
    let alive = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(LatchedShutdownFactory {
        latch: Arc::clone(&latch),
        alive: Arc::clone(&alive),
        built: AtomicUsize::new(0),
    });
    let manager = TenantRuntimeManager::new(
        deadline_config(CAP, 8, SLOTS),
        registry,
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );

    for name in ["t0", "t1", "t2", "t3"] {
        drop(
            manager
                .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
                .await
                .expect("resolves while a teardown slot is free"),
        );
        assert!(
            alive.load(Ordering::SeqCst) <= CAP + SLOTS,
            "live runtimes must stay within the cache plus the teardown slots, saw {}",
            alive.load(Ordering::SeqCst)
        );
        // Space the resolves so the millisecond last-used stamps order the LRU.
        sleep(Duration::from_millis(3)).await;
    }
    pretty_assertions::assert_eq!(alive.load(Ordering::SeqCst), CAP + SLOTS, "two cached, two stalled in teardown");
    pretty_assertions::assert_eq!(manager.runtimes.len(), CAP);

    let refused = manager.resolve_state(&TenantRef::Named(named("t4")), TenantCreationPolicy::MustExist).await;
    assert!(
        matches!(
            refused,
            Err(TenancyError::Saturated {
                wait: SaturatedWait::TeardownSlot,
                ..
            })
        ),
        "a cold build that cannot free a slot in time is refused as saturated"
    );
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 4, "the refused build never ran");
    pretty_assertions::assert_eq!(alive.load(Ordering::SeqCst), CAP + SLOTS, "no runtime was added past the bound");

    // Let the stalled teardowns finish: the same request is admitted now.
    latch.close();
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(alive.load(Ordering::SeqCst), CAP);
    let admitted = manager.resolve_state(&TenantRef::Named(named("t4")), TenantCreationPolicy::MustExist).await;
    assert!(admitted.is_ok(), "once a teardown slot frees, the cold build is admitted");
    drop(admitted);
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(alive.load(Ordering::SeqCst), CAP, "the cache is back inside its cap");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_waiting_on_another_requests_build_is_refused_at_the_admission_deadline() {
    // A second request for a tenant whose runtime is still being built waits on
    // the single-flight lock. The wait answers to the admission deadline: past
    // it the request is refused as saturated (503 + Retry-After at the HTTP
    // layer) rather than stalling into the request timeout.
    let registry = Arc::new(FakeRegistry::new(&["acme"]));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::new(AtomicUsize::new(0)),
        gate: Arc::clone(&gate),
    });
    let manager = TenantRuntimeManager::new(
        deadline_config(64, 8, 8),
        registry,
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );
    let tenant = TenantRef::Named(named("acme"));

    let building = {
        let manager = Arc::clone(&manager);
        let tenant = tenant.clone(); // clone: the task owns its reference
        tokio::spawn(async move { manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.map(|leased| leased.state) })
    };
    // Let the first request take the lock and enter its build.
    sleep(Duration::from_millis(50)).await;
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 1);

    let waiting = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
    assert!(
        matches!(
            waiting,
            Err(TenancyError::Saturated {
                wait: SaturatedWait::BuildLock,
                ..
            })
        ),
        "a request still waiting on another's build at the deadline is refused as saturated"
    );

    gate.notify_waiters();
    let built = building.await.expect("task joins");
    assert!(built.is_ok(), "the build the refused request waited on still completes");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 1, "the refused request never built");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cold_build_waiting_for_a_build_permit_is_refused_at_the_admission_deadline() {
    // With one build permit held by a slow build of another tenant, a cold build
    // waits for the permit only until the admission deadline, then is refused as
    // saturated without building.
    let registry = Arc::new(FakeRegistry::new(&["slow", "queued"]));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::new(AtomicUsize::new(0)),
        gate: Arc::clone(&gate),
    });
    let manager = TenantRuntimeManager::new(
        deadline_config(64, 1, 8),
        registry,
        Arc::clone(&factory),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
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

    let queued = manager.resolve_state(&TenantRef::Named(named("queued")), TenantCreationPolicy::MustExist).await;
    assert!(
        matches!(
            queued,
            Err(TenancyError::Saturated {
                wait: SaturatedWait::BuildPermit,
                ..
            })
        ),
        "a cold build still waiting for a permit at the deadline is refused as saturated"
    );
    pretty_assertions::assert_eq!(factory.built.load(Ordering::SeqCst), 1, "the refused cold build never ran");

    gate.notify_waiters();
    assert!(slow.await.expect("task joins").is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tenant_suspended_mid_request_is_refused_then_released_when_the_request_ends() {
    // Suspending a tenant whose runtime a request still holds cannot release it
    // at once. The runtime is retired instead: admission refuses the tenant as
    // nonexistent even though its runtime is still cached (clause 5.5.10), and
    // when the request's lease drops the janitor releases it promptly rather
    // than leaving a suspended tenant's runtime, and its background work,
    // running until it idles out.
    let Harness { manager, shutdowns, .. } = harness(&["acme"], true, 64);
    let janitor = manager.spawn_janitor().expect("a multi-tenant broker runs a janitor");
    let tenant = TenantRef::Named(named("acme"));
    let in_flight = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

    let view = manager
        .set_admin_status(&named("acme"), TenantStatus::Suspended)
        .await
        .expect("the suspend is applied");
    assert!(view.is_some(), "the suspended tenant is still registered");
    assert!(manager.runtimes.contains_key(&tenant), "the leased runtime cannot be released yet");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::SeqCst), 0);

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await;
    assert!(
        matches!(refused, Err(TenancyError::UnknownTenant { .. })),
        "a retired runtime is never handed out, even from the cache"
    );

    drop(in_flight);
    // The janitor's next tick is thirty seconds out, so a release inside this
    // window can only be the lease's own release signal.
    timeout(Duration::from_secs(2), async {
        while manager.runtimes.contains_key(&tenant) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the retired runtime is released as soon as its last lease drops");
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::SeqCst), 1, "the released runtime is torn down");
    janitor.abort();
}

#[tokio::test]
async fn a_tenant_resumed_while_its_retired_runtime_is_leased_is_served_again() {
    // A resume that lands before the suspended tenant's last lease dropped
    // returns the still-cached runtime to service rather than refusing a tenant
    // that is active again.
    let Harness { manager, factory, .. } = harness(&["acme"], true, 64);
    let tenant = TenantRef::Named(named("acme"));
    let in_flight = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

    manager
        .set_admin_status(&named("acme"), TenantStatus::Suspended)
        .await
        .expect("the suspend is applied");
    manager
        .set_admin_status(&named("acme"), TenantStatus::Active)
        .await
        .expect("the resume is applied");

    let served = manager
        .resolve_state(&tenant, TenantCreationPolicy::MustExist)
        .await
        .expect("a resumed tenant is served");
    assert!(Arc::ptr_eq(&served.state, &in_flight.state), "the reinstated runtime is reused");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "no second runtime was built");
}

#[tokio::test]
async fn suspending_an_unleased_tenant_releases_its_runtime_before_answering() {
    // With no request holding it, a suspend tears the runtime down before it
    // returns, so the caller's 204 means the tenant's pool is already closed.
    let Harness { manager, shutdowns, .. } = harness(&["acme"], true, 64);
    let tenant = TenantRef::Named(named("acme"));
    drop(manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves"));

    manager
        .set_admin_status(&named("acme"), TenantStatus::Suspended)
        .await
        .expect("the suspend is applied");
    assert!(!manager.runtimes.contains_key(&tenant), "the runtime is released synchronously");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::SeqCst), 1, "its teardown completed before the suspend returned");
}

/// Whether the cached runtime of `tenant` serves an established tenant.
fn cached_existence(manager: &TenantRuntimeManager<FakeRegistry, FakeFactory>, tenant: &TenantRef) -> Option<TenantExistence> {
    manager.runtimes.get(tenant).map(|cached| cached.existence())
}

/// Clause 5.5.10: a tenant is created when an operation creating information
/// targets it, so the tenant a creating request provisioned does not exist for
/// a request that requires it until its database holds information.
#[tokio::test]
async fn a_tenant_created_on_demand_is_answered_as_nonexistent_until_its_database_holds_information() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("acme"));

    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("provisions"));
    pretty_assertions::assert_eq!(cached_existence(&manager, &tenant), Some(TenantExistence::Provisional));

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
    assert!(
        matches!(refused, Err(TenancyError::UnknownTenant { .. })),
        "an empty provisional tenant does not exist"
    );

    registry.occupy("acme");
    let served = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("established");
    pretty_assertions::assert_eq!(served.state.as_str(), "db_acme");
    pretty_assertions::assert_eq!(cached_existence(&manager, &tenant), Some(TenantExistence::Established));
    assert!(registry.all().contains(&"acme".to_string()), "the registry records the tenant as established");
}

#[tokio::test]
async fn a_cached_runtime_of_an_established_tenant_is_served_without_reading_its_database_again() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("acme"));
    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("provisions"));
    registry.occupy("acme");
    drop(manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("established"));
    let establishments = registry.establishments.load(Ordering::Relaxed);

    for _ in 0..3 {
        drop(manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("served"));
    }
    pretty_assertions::assert_eq!(registry.establishments.load(Ordering::Relaxed), establishments);
}

#[tokio::test]
async fn a_creating_request_on_a_cached_provisional_tenant_is_served_without_settling_it() {
    let Harness {
        manager, registry, factory, ..
    } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("acme"));
    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("provisions"));

    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("served"));
    pretty_assertions::assert_eq!(registry.establishments.load(Ordering::Relaxed), 0);
    pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 1, "a cached runtime is not provisioned again");
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn an_empty_provisional_tenant_with_no_runtime_is_refused_and_never_built() {
    let Harness {
        manager, registry, factory, ..
    } = harness(&[], true, 64);
    registry.register_provisional("acme");
    let tenant = TenantRef::Named(named("acme"));

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
    assert!(matches!(refused, Err(TenancyError::UnknownTenant { .. })));
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 0);
    assert!(!manager.runtimes.contains_key(&tenant));
}

#[tokio::test]
async fn a_provisional_tenant_holding_information_is_established_by_a_cold_admission() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    registry.register_provisional("acme");
    registry.occupy("acme");
    let tenant = TenantRef::Named(named("acme"));

    drop(manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("established"));
    pretty_assertions::assert_eq!(cached_existence(&manager, &tenant), Some(TenantExistence::Established));
    pretty_assertions::assert_eq!(
        registry.provisioned.load(Ordering::Relaxed),
        0,
        "a request that requires the tenant never provisions it"
    );
}

/// An interrupted provisioning leaves a provisional row whose database may be
/// missing, so a creating request that finds one prepares it again.
#[tokio::test]
async fn a_creating_request_on_an_uncached_provisional_tenant_provisions_it_again() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    registry.register_provisional("acme");
    let tenant = TenantRef::Named(named("acme"));

    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("provisions"));
    pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 1);
    pretty_assertions::assert_eq!(cached_existence(&manager, &tenant), Some(TenantExistence::Provisional));
}

#[tokio::test]
async fn the_sweep_offers_only_provisional_tenants_this_broker_holds_no_runtime_for() {
    let Harness { manager, registry, .. } = harness(&["established"], true, 64);
    drop(
        manager
            .resolve_state(&TenantRef::Named(named("cached")), TenantCreationPolicy::CreateOnDemand)
            .await
            .expect("provisions"),
    );
    registry.register_provisional("abandoned");

    pretty_assertions::assert_eq!(manager.settle_provisional().await, ProvisionalSettlement { established: 0, retired: 1 });
    pretty_assertions::assert_eq!(registry.offered(), vec!["abandoned".to_string()]);
    pretty_assertions::assert_eq!(registry.provisional(), vec!["cached".to_string()]);
}

/// A tenant only ever written to is never admitted by a request that requires
/// it to exist, so the sweep is what establishes it while its runtime stays
/// cached.
#[tokio::test]
async fn the_sweep_establishes_a_cached_provisional_tenant_whose_database_holds_information() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("writer"));
    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("provisions"));
    registry.occupy("writer");

    pretty_assertions::assert_eq!(manager.settle_provisional().await, ProvisionalSettlement { established: 1, retired: 0 });
    pretty_assertions::assert_eq!(registry.offered(), Vec::<String>::new(), "a cached tenant is never offered for retirement");
    pretty_assertions::assert_eq!(registry.provisional(), Vec::<String>::new());
    pretty_assertions::assert_eq!(cached_existence(&manager, &tenant), Some(TenantExistence::Established));
}

#[tokio::test]
async fn the_sweep_keeps_a_cached_provisional_tenant_whose_database_is_empty() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("refused"));
    drop(manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("provisions"));

    pretty_assertions::assert_eq!(manager.settle_provisional().await, ProvisionalSettlement::default());
    pretty_assertions::assert_eq!(registry.provisional(), vec!["refused".to_string()]);
    pretty_assertions::assert_eq!(cached_existence(&manager, &tenant), Some(TenantExistence::Provisional));
}

#[tokio::test]
async fn the_sweep_retires_nothing_when_the_registry_cannot_be_read() {
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::new(UnreadableProvisionalRegistry),
        Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0)))),
        Arc::new(FakeProbe::none()),
        Arc::new(FakeSchedule::inert()),
    );
    pretty_assertions::assert_eq!(manager.settle_provisional().await, ProvisionalSettlement::default());
}

/// A registry that lists no provisional tenant because it cannot be read.
struct UnreadableProvisionalRegistry;

#[async_trait]
impl TenantDatabase for UnreadableProvisionalRegistry {
    fn default_database(&self) -> TenantDatabaseName {
        TenantDatabaseName::from("andromeda".to_string())
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        Ok(Vec::new())
    }

    async fn registered_tenant(&self, _name: &TenantName) -> Result<Option<RegisteredTenant>, TenancyError> {
        Ok(None)
    }

    async fn list_tenant_records(&self, _start: &TenantPageStart, _limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
        Ok((Vec::new(), 0))
    }

    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        Ok(TenantDatabaseName::from(format!("db_{}", name.as_str())))
    }

    async fn establish(&self, _name: &TenantName) -> Result<Establishment, TenancyError> {
        Ok(Establishment::Empty)
    }

    async fn provisional_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        Err(TenancyError::RegistryUnavailable(PersistenceFailure::Unavailable))
    }

    async fn retire_provisional(&self, _name: &TenantName, _idle_for: Duration) -> Result<Retirement, TenancyError> {
        Ok(Retirement::Retired)
    }
}

#[tokio::test]
async fn the_sweep_establishes_a_provisional_tenant_whose_database_holds_information() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    registry.register_provisional("acme");
    registry.occupy("acme");

    pretty_assertions::assert_eq!(manager.settle_provisional().await, ProvisionalSettlement { established: 1, retired: 0 });
    assert!(registry.all().contains(&"acme".to_string()));
    pretty_assertions::assert_eq!(registry.provisional(), Vec::<String>::new());
}

#[tokio::test]
async fn the_tenant_collection_lists_a_provisional_tenant_holding_information_and_not_an_empty_one() {
    let Harness { manager, registry, .. } = harness(&["established"], true, 64);
    registry.register_provisional("created");
    registry.occupy("created");
    registry.register_provisional("refused");

    let listing = manager.list_tenants(&TenantPageStart::Forward { after: None }, 10).await.expect("lists");
    let names: Vec<&str> = listing.views.iter().map(|view| view.record.tenant_name.as_str()).collect();
    pretty_assertions::assert_eq!(names, vec!["created", "established"]);
    pretty_assertions::assert_eq!(listing.total, 2);
}

#[tokio::test]
async fn a_tenant_read_settles_a_provisional_tenant() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    registry.register_provisional("refused");
    registry.register_provisional("created");
    registry.occupy("created");

    assert!(manager.tenant(&named("refused")).await.expect("reads").is_none());
    let created = manager.tenant(&named("created")).await.expect("reads").expect("exists");
    pretty_assertions::assert_eq!(created.record.status, TenantStatus::Active);
}

#[tokio::test]
async fn suspending_an_empty_provisional_tenant_finds_no_tenant() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    registry.register_provisional("refused");

    let view = manager.set_admin_status(&named("refused"), TenantStatus::Suspended).await.expect("answers");
    assert!(view.is_none());
    assert!(!registry.is_suspended("refused"));
}

/// A creating operation may commit work that falls due before any request
/// establishes its tenant, and that work runs regardless.
#[tokio::test]
async fn a_provisional_tenant_with_work_due_is_woken_and_stays_provisional() {
    let registry = Arc::new(FakeRegistry::new(&[]));
    registry.register_provisional("acme");
    let factory = Arc::new(FakeFactory::new(Arc::new(AtomicUsize::new(0))));
    let probe = Arc::new(FakeProbe::due_as_answered());
    let manager = TenantRuntimeManager::new(
        config(true, 64),
        Arc::clone(&registry),
        factory,
        Arc::clone(&probe) as Arc<dyn PendingWorkProbe>,
        Arc::new(FakeSchedule::inert()),
    );

    pretty_assertions::assert_eq!(manager.wake(&named("acme")).await, WakeOutcome::Loaded);
    pretty_assertions::assert_eq!(probe.asked(), vec![TenantExistence::Provisional]);
    pretty_assertions::assert_eq!(cached_existence(&manager, &TenantRef::Named(named("acme"))), Some(TenantExistence::Provisional));
}
