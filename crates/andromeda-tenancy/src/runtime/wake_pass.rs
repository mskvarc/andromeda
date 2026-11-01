//! One pass of the wake loop: the tenants their wake schedule holds due, then,
//! in what those leave of the pass budget, the tenants behind the server's
//! extension versions.
//!
//! Scheduled wakes are claimed first and always fit, so catching up never
//! delays a tenant whose work has fallen due, and the two together never load
//! more than `max_wakes_per_pass` tenants. A catch-up load is not due work: it
//! takes only a free cache slot and never unloads another tenant to make room,
//! so it cannot push out a tenant a request is using.

use crate::{
    catch_up::{CatchUpCandidates, CatchUpError},
    database::TenantDatabase,
    registered_tenant::RegisteredTenant,
    runtime::{TenantRuntimeManager, cached_runtime::RuntimeExistence},
    state_factory::TenantStateFactory,
    status::TenantStatus,
    tenant_ref::TenantRef,
    wake_schedule::WakeScheduleError,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::record_wake;
use std::time::SystemTime;
use thiserror::Error;

/// Why a wake pass stopped before it was done.
#[derive(Debug, Error)]
pub enum WakePassError {
    /// The due tenants could not be claimed.
    #[error(transparent)]
    Schedule(#[from] WakeScheduleError),
    /// The tenants behind the server's extension versions could not be read.
    #[error(transparent)]
    CatchUp(#[from] CatchUpError),
}

/// How many tenants one wake pass took up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakePassReport {
    /// Tenants claimed from the wake schedule and woken.
    pub scheduled: usize,
    /// Tenants loaded to bring their extensions up to date.
    pub caught_up: usize,
}

/// What one catch-up candidate came to.
enum CatchUpAttempt {
    /// A load was attempted, whether or not it succeeded, and spends budget.
    Attempted,
    /// The tenant is resident, or not an active established tenant; no budget
    /// is spent on it.
    Skipped,
    /// The cache has no free slot, so the pass loads nothing more.
    CacheFull,
}

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Wake the tenants due at `now`, leasing each claim to `lease_until`, then
    /// spend what is left of the pass budget on the tenants `catch_up` lists.
    ///
    /// # Errors
    ///
    /// Returns [`WakePassError::Schedule`] when the claim fails, before any
    /// tenant is loaded, and [`WakePassError::CatchUp`] when the catch-up read
    /// fails, after the scheduled wakes ran.
    pub async fn run_wake_pass(&self, catch_up: &dyn CatchUpCandidates, now: SystemTime, lease_until: SystemTime) -> Result<WakePassReport, WakePassError> {
        let budget = self.waker.max_wakes_per_pass();
        let due = self.schedule.claim_due(now, lease_until, budget).await?;
        for tenant in &due {
            self.wake(tenant).await;
        }
        let remaining = budget.saturating_sub(due.len());
        let caught_up = if remaining == 0 {
            0
        } else {
            self.catch_up(catch_up.lagging_tenants().await?, remaining).await
        };
        Ok(WakePassReport {
            scheduled: due.len(),
            caught_up,
        })
    }

    /// Load `candidates` in order until `budget` loads were attempted or the
    /// cache has no free slot, returning how many were attempted.
    async fn catch_up(&self, candidates: Vec<TenantName>, budget: usize) -> usize {
        let mut attempted = 0;
        for tenant in candidates {
            if attempted == budget {
                break;
            }
            match self.load_to_catch_up(&tenant).await {
                CatchUpAttempt::Attempted => attempted += 1,
                CatchUpAttempt::Skipped => {}
                CatchUpAttempt::CacheFull => break,
            }
        }
        attempted
    }

    /// Load one lagging tenant without probing it first: its open runs the
    /// extension update and records the versions it leaves, which is the work
    /// it was chosen for.
    async fn load_to_catch_up(&self, tenant: &TenantName) -> CatchUpAttempt {
        let tenant_ref = TenantRef::Named(tenant.clone()); // clone: the ref outlives this call
        if self.runtimes.contains_key(&tenant_ref) {
            return CatchUpAttempt::Skipped;
        }
        if self.runtimes.len() >= self.config.residency.max_resident.get() {
            return CatchUpAttempt::CacheFull;
        }
        let database = match self.database.registered_tenant(tenant).await {
            Ok(Some(RegisteredTenant::Established(record))) => match record.status {
                TenantStatus::Active => record.database,
                TenantStatus::Suspended => return CatchUpAttempt::Skipped,
            },
            Ok(Some(RegisteredTenant::Provisional { .. }) | None) | Err(_) => return CatchUpAttempt::Skipped,
        };
        record_wake(self.build_woken(&tenant_ref, database, RuntimeExistence::Established, tenant).await);
        CatchUpAttempt::Attempted
    }
}
