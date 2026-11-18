//! Where a tenant's earliest outstanding obligation is read from, for the
//! background pacer and the wake loop to decide when its timers are owed work.

use andromeda_postgresql_context_store::{error::ContextStoreError, pending_work::earliest_obligation};
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
}

impl TenantObligations {
    /// Read obligations over `pool`.
    pub(crate) fn new(pool: &PgPool) -> TenantObligations {
        TenantObligations { pool: pool.clone() }
    }
}

#[async_trait]
impl ObligationSource for TenantObligations {
    async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
        earliest_obligation(&self.pool).await
    }
}
