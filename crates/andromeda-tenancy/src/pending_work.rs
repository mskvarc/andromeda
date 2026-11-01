//! Whether a tenant that is not in the cache still has background work due.
//!
//! A dormant tenant answers no requests, so nothing touches its runtime and its
//! clause-5.8.6 periodic notifications, durable-delivery retries, snapshot
//! preparation and status outbox, pending side effects, temporal append
//! intents, `expiresAt` sweeps, and the columnstore conversion and retention of
//! its history would otherwise never run. The wake loop asks this port for the
//! single earliest instant a tenant has an obligation due, and loads the
//! tenant only when one exists. The port is SQL-free: which of those
//! obligations is outstanding is each store's concern, every store answers for
//! its own, and the cache needs only the instant.

use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use std::time::SystemTime;
use thiserror::Error;

/// The earliest background work a tenant has due, if any.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingWork {
    /// Nothing is outstanding: no retry, no expiry, no periodic notification,
    /// no snapshot preparation, no history past a sweep horizon.
    None,
    /// The earliest instant at which some obligation becomes due.
    ///
    /// May be in the past (already overdue, so the tenant is woken now) or
    /// in the future, in which case the wake schedule records it and the tenant
    /// is left dormant until then.
    DueAt(SystemTime),
}

/// Why a pending-work probe could not answer.
///
/// Carries the database it was probing rather than a tenant name because the
/// probe is handed the database directly; the caller pairs it back to a tenant.
/// The detail is the one place this port holds unstructured text: it is
/// whatever the store reported and this crate cannot name.
#[derive(Debug, Error)]
pub enum PendingWorkError {
    /// The probe's own connection could not be acquired within the deadline.
    ///
    /// Backpressure, not a fault: the pooler or backend cap left no connection
    /// for the probe. The caller records a retry instant rather than treating
    /// the tenant as having nothing due.
    #[error("the pending-work probe for {database} was saturated")]
    Saturated {
        /// The database whose probe could not acquire a connection.
        database: TenantDatabaseName,
    },
    /// The probe statement failed for a reason other than saturation.
    #[error("the pending-work probe for {database} failed: {cause}")]
    ProbeFailed {
        /// The database whose probe failed.
        database: TenantDatabaseName,
        /// How the store failed.
        #[source]
        cause: PersistenceFailure,
    },
    /// The tenant's database holds a schema older than this broker's, or none
    /// yet: it is not migrated.
    ///
    /// Not a fault. A provisioning that created the database is still
    /// migrating it, or this broker's release reads a tenant no runtime of the
    /// release has opened yet. The probe cannot read obligations from a schema
    /// it does not know, so the caller decides by the tenant's standing.
    #[error("the pending-work probe for {database} found its schema not migrated")]
    Unmigrated {
        /// The database whose schema is not migrated.
        database: TenantDatabaseName,
    },
    /// The tenant's database no longer exists.
    ///
    /// The tenant was deprovisioned (or its database dropped out from under the
    /// broker) while a stale wake-schedule row survived. This is permanent, not
    /// backpressure: the caller clears the schedule rather than re-arming a
    /// retry, so a gone tenant does not churn the wake pass forever.
    #[error("the pending-work probe for {database} found no such database")]
    DatabaseGone {
        /// The database that no longer exists.
        database: TenantDatabaseName,
    },
}

/// Reads a tenant's own database for the earliest instant it has work due,
/// without loading the tenant's runtime.
///
/// Declared here, beside the runtime cache that calls it, rather than beside
/// its implementation: whether "work" is a durable-notification retry, an
/// expiry sweep, a periodic notification or a snapshot preparation is the
/// store's decision, and the wake policy must not depend on which. The probe
/// runs a single index-backed read over a transient connection, so it is safe
/// through a transaction pooler and adds no permanent backend.
#[async_trait]
pub trait PendingWorkProbe: Send + Sync + 'static {
    /// The earliest instant `database` has background work due.
    ///
    /// # Errors
    ///
    /// Returns [`PendingWorkError::Saturated`] when the probe's connection
    /// cannot be acquired within the deadline,
    /// [`PendingWorkError::DatabaseGone`] when the database does not exist,
    /// [`PendingWorkError::Unmigrated`] when its schema is not migrated, and
    /// [`PendingWorkError::ProbeFailed`] when the read itself fails.
    #[must_use = "a pending-work probe must be awaited"]
    async fn earliest_pending_work(&self, database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError>;
}
