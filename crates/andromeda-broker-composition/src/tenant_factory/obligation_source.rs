//! Where a tenant's earliest outstanding obligation is read from, for the
//! background pacer and the wake loop to decide when its timers are owed work.

use andromeda_postgresql_context_store::{error::ContextStoreError, pending_work::earliest_obligation};
use andromeda_subscription_forwarding::receiver::ReceiverEndpoint;
use async_trait::async_trait;
use sqlx::PgPool;
use std::time::SystemTime;

/// Where a tenant's earliest outstanding obligation is read from.
#[async_trait]
pub(crate) trait ObligationSource: Send + Sync + 'static {
    /// The earliest instant any obligation falls due, or `None` when nothing is
    /// outstanding.
    async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError>;
}

/// The tenant's own database, read over its serving pool.
pub(crate) struct TenantObligations {
    /// The tenant's serving pool.
    pool: PgPool,
    /// The receiver the distributed subscription side has context sources
    /// notify, `None` when it does not run.
    forwarding: Option<ReceiverEndpoint>,
}

impl TenantObligations {
    /// Read obligations over `pool`, those of the distributed subscription
    /// side among them when it notifies context sources at `forwarding`.
    pub(crate) fn new(pool: &PgPool, forwarding: Option<ReceiverEndpoint>) -> TenantObligations {
        TenantObligations {
            pool: pool.clone(),
            forwarding,
        }
    }
}

#[async_trait]
impl ObligationSource for TenantObligations {
    async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
        earliest_obligation(&self.pool, self.forwarding.as_ref()).await
    }
}
