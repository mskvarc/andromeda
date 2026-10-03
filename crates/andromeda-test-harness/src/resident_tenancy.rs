//! A tenant runtime cache over one fixed state, for unit tests of route
//! services that hold a genuine [`RuntimeLease`](andromeda_tenancy::runtime::lease::RuntimeLease).
//!
//! Every per-request capability set carries the lease that keeps its tenant's
//! runtime resident, and a lease is only ever handed out by a
//! [`TenantRuntimeManager`]. A test that drives a route through its resolver
//! therefore resolves through a real manager, whose registry holds the unnamed
//! default tenant and the named tenants the test lists, and whose factory hands
//! every tenant the same state.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::{
    config::TenancyConfig,
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    error::TenancyError,
    pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
    runtime::{TenantRuntimeManager, lease::LeasedState},
    state_factory::{RuntimeActivity, TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_page_start::TenantPageStart,
    tenant_record::TenantRecord,
    tenant_ref::TenantRef,
    wake_schedule::{TenantWakeSchedule, WakeScheduleError},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::SystemTime};

/// A runtime cache whose every tenant is served the same state.
pub struct ResidentTenancy<S: Send + Sync + 'static> {
    /// The manager the leases come from.
    manager: Arc<TenantRuntimeManager<FixedRegistry, FixedStateFactory<S>>>,
}

impl<S: Send + Sync + 'static> ResidentTenancy<S> {
    /// A cache serving `state` to the unnamed default tenant and to each of
    /// `named`; any other named tenant is unknown.
    #[must_use]
    pub fn new(state: Arc<S>, named: Vec<TenantName>) -> ResidentTenancy<S> {
        let manager = TenantRuntimeManager::new(
            TenancyConfig::default(),
            Arc::new(FixedRegistry { named }),
            Arc::new(FixedStateFactory { state }),
            Arc::new(NothingDue),
            Arc::new(NoWakes),
        );
        ResidentTenancy { manager }
    }

    /// The state for `tenant` with the lease that keeps its runtime resident.
    ///
    /// # Errors
    ///
    /// Returns [`TenancyError::UnknownTenant`] for a named tenant the cache was
    /// not given, as the broker's own cache does for an unregistered one.
    pub async fn resolve(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<LeasedState<S>, TenancyError> {
        self.manager.resolve_state(tenant, policy).await
    }
}

/// A registry holding a fixed set of named tenants, none of them suspended.
struct FixedRegistry {
    /// The registered named tenants.
    named: Vec<TenantName>,
}

impl FixedRegistry {
    /// The registry row of a registered tenant.
    fn record(name: &TenantName) -> TenantRecord {
        TenantRecord::new(
            name.clone(), // clone: the record owns its name
            TenantStatus::Active,
            TenantDatabaseName::from(format!("resident_{}", name.as_str())),
            DateTime::<Utc>::UNIX_EPOCH,
            DateTime::<Utc>::UNIX_EPOCH,
        )
    }
}

#[async_trait]
impl TenantDatabase for FixedRegistry {
    fn default_database(&self) -> TenantDatabaseName {
        TenantDatabaseName::from("resident".to_owned())
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        Ok(self.named.clone()) // clone: the caller owns the listing
    }

    async fn tenant_record(&self, name: &TenantName) -> Result<Option<TenantRecord>, TenancyError> {
        Ok(self.named.contains(name).then(|| FixedRegistry::record(name)))
    }

    async fn list_tenant_records(&self, _start: &TenantPageStart, _limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
        let records: Vec<TenantRecord> = self.named.iter().map(FixedRegistry::record).collect();
        let total = i64::try_from(records.len()).unwrap_or(i64::MAX);
        Ok((records, total))
    }

    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        Err(TenancyError::UnknownTenant {
            name: name.clone(), // clone: the error owns the name it reports
        })
    }
}

/// A factory handing every tenant the same state.
struct FixedStateFactory<S> {
    /// The state every tenant is served.
    state: Arc<S>,
}

#[async_trait]
impl<S: Send + Sync + 'static> TenantStateFactory for FixedStateFactory<S> {
    type State = S;
    type Runtime = InertRuntime;

    async fn build(&self, _tenant: &TenantRef, _database: &TenantDatabaseName) -> Result<(Arc<S>, InertRuntime), TenancyError> {
        Ok((Arc::clone(&self.state), InertRuntime))
    }
}

/// A runtime holding nothing open.
struct InertRuntime;

#[async_trait]
impl TenantRuntime for InertRuntime {
    async fn shutdown(self) {}

    fn activity(&self) -> Arc<dyn RuntimeActivity> {
        Arc::new(InertRuntime)
    }
}

impl RuntimeActivity for InertRuntime {
    fn record_activity(&self) {}
}

/// A probe finding nothing due in any tenant.
struct NothingDue;

#[async_trait]
impl PendingWorkProbe for NothingDue {
    async fn earliest_pending_work(&self, _database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError> {
        Ok(PendingWork::None)
    }
}

/// A wake schedule that records nothing and claims nothing.
struct NoWakes;

#[async_trait]
impl TenantWakeSchedule for NoWakes {
    async fn record(&self, _tenant: &TenantName, _probed_at: SystemTime, _wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        Ok(())
    }

    async fn claim_due(&self, _now: SystemTime, _lease_until: SystemTime, _limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        Ok(Vec::new())
    }

    async fn settle(&self, _tenant: &TenantName, _probed_at: SystemTime, _wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        Ok(())
    }

    async fn next_unprobed(&self, _limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use crate::resident_tenancy::ResidentTenancy;
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, tenant_ref::TenantRef};
    use std::sync::Arc;

    fn tenant(name: &str) -> TenantName {
        name.parse().expect("a valid tenant name")
    }

    #[tokio::test]
    async fn the_default_and_listed_tenants_resolve_to_the_fixed_state_and_others_are_unknown() {
        let tenancy = ResidentTenancy::new(Arc::new(7_u8), vec![tenant("acme")]);

        let default = tenancy
            .resolve(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
            .await
            .expect("the default tenant");
        let acme = tenancy
            .resolve(&TenantRef::Named(tenant("acme")), TenantCreationPolicy::MustExist)
            .await
            .expect("a listed tenant");
        let other = tenancy.resolve(&TenantRef::Named(tenant("other")), TenantCreationPolicy::MustExist).await;

        pretty_assertions::assert_eq!((*default.state, *acme.state), (7, 7));
        assert!(matches!(other, Err(TenancyError::UnknownTenant { .. })), "an unlisted tenant is unknown");
    }
}
