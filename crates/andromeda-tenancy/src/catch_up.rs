//! Finding the dormant tenants whose database runs older extension versions
//! than the server ships.
//!
//! A tenant database keeps the extension versions it was created or last
//! updated at until a broker opens it, and opening it is when the update runs.
//! After an image upgrade, a tenant no request opens would stay behind, its
//! first request would wait for the update, and an image that stops shipping
//! the old library would leave it unopenable. The wake pass therefore spends
//! what its scheduled wakes leave of its budget on loading these tenants, the
//! furthest behind first. The store that records each tenant's versions knows
//! which tenants are behind and in what order; this crate only loads them.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use thiserror::Error;

/// Why the tenants behind the server's extension versions could not be read.
#[derive(Debug, Error)]
pub enum CatchUpError {
    /// The platform read failed.
    ///
    /// The versions live in the platform database and this crate cannot name
    /// its dialect, so the failure arrives classified.
    #[error("the tenants behind the server's extension versions could not be read: {0}")]
    Store(#[source] PersistenceFailure),
}

/// The tenants a wake pass loads to bring their extensions up to date.
#[async_trait]
pub trait CatchUpCandidates: Send + Sync + 'static {
    /// The active named tenants with no wake pending whose database is behind
    /// the extension versions the server ships, furthest behind first.
    ///
    /// A tenant whose last open already tried to update against the versions
    /// the server ships now is not listed until the server ships newer ones,
    /// so a tenant whose role may not update an extension is loaded once per
    /// server version rather than on every pass.
    ///
    /// # Errors
    ///
    /// Returns [`CatchUpError::Store`] when the read fails.
    #[must_use = "reading the tenants to catch up must be awaited"]
    async fn lagging_tenants(&self) -> Result<Vec<TenantName>, CatchUpError>;
}
