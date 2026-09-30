//! The persistent, fleet-shared record of when each tenant should next be woken.
//!
//! One broker replica evicts a dormant tenant and records the instant its next
//! obligation falls due; any replica may later claim that row and wake the
//! tenant. The schedule lives in the platform database, not in a broker's
//! memory, so a wake survives the eviction of the runtime that scheduled it and
//! is not duplicated across replicas. Claims are single-statement
//! `FOR UPDATE SKIP LOCKED` reservations: no replica takes a fleet-wide lock,
//! and a replica that crashes mid-wake leaves a leased row that re-fires once
//! the lease lapses.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use std::time::SystemTime;
use thiserror::Error;

/// Why a wake-schedule operation could not complete.
#[derive(Debug, Error)]
pub enum WakeScheduleError {
    /// The platform statement failed.
    ///
    /// The schedule lives in the platform database and this crate cannot name
    /// its dialect, so the failure arrives classified.
    #[error("the tenant wake schedule could not be updated: {0}")]
    Store(#[source] PersistenceFailure),
}

/// The persistent schedule of tenant wakes, shared across replicas.
///
/// Implemented by the store that owns the platform database. Every method is a
/// single statement so it composes with a transaction pooler and holds no lock
/// across an await.
#[async_trait]
pub trait TenantWakeSchedule: Send + Sync + 'static {
    /// Record that `tenant` was probed at `probed_at`, and that its next
    /// obligation -- if any -- falls due at `wake_at`.
    ///
    /// `wake_at` of `None` means the tenant had nothing due and should not be
    /// woken until a later probe finds work. Upserts the row so a re-probe
    /// overwrites a stale schedule.
    ///
    /// # Errors
    ///
    /// Returns [`WakeScheduleError::Store`] when the upsert fails.
    #[must_use = "recording a wake schedule must be awaited"]
    async fn record(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError>;

    /// Claim up to `limit` tenants whose `wake_at` is at or before `now`,
    /// leasing each claimed row forward to `lease_until` so a crash before the
    /// wake settles re-fires it once the lease lapses.
    ///
    /// The claim is a single `FOR UPDATE SKIP LOCKED` update, so concurrent
    /// replicas never claim the same tenant and never block one another.
    ///
    /// # Errors
    ///
    /// Returns [`WakeScheduleError::Store`] when the claim fails.
    #[must_use = "claiming due wakes must be awaited"]
    async fn claim_due(&self, now: SystemTime, lease_until: SystemTime, limit: usize) -> Result<Vec<TenantName>, WakeScheduleError>;

    /// Settle a claimed tenant after its wake attempt: record the new probe
    /// instant and the next `wake_at`, clearing the claim lease.
    ///
    /// # Errors
    ///
    /// Returns [`WakeScheduleError::Store`] when the update fails.
    #[must_use = "settling a wake must be awaited"]
    async fn settle(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError>;

    /// The registered tenants least recently probed, up to `limit`, for the
    /// crash-recovery reconciliation sweep to round-robin over.
    ///
    /// Includes tenants with no schedule row yet (never probed), which sort
    /// ahead of any probed tenant.
    ///
    /// # Errors
    ///
    /// Returns [`WakeScheduleError::Store`] when the read fails.
    #[must_use = "reading unprobed tenants must be awaited"]
    async fn next_unprobed(&self, limit: usize) -> Result<Vec<TenantName>, WakeScheduleError>;
}
